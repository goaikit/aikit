//! One explicit policy using the existing JSON ownership and recovery journal.
use super::*;

const CAP_KEY: &str = "CLAUDE_CODE_STOP_HOOK_BLOCK_CAP";
const CAP_POINTER: &str = "/env/CLAUDE_CODE_STOP_HOOK_BLOCK_CAP";

/// Requested configuration, not qualification. NativeDefault preserves the
/// legacy installer behavior; RequireUnbounded requests removal of a native
/// continuation limit. The effective setting and behavior need native evidence.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompletionBlockingPolicy {
    #[default]
    NativeDefault,
    RequireUnbounded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingChange {
    pub pointer: String,
    /// None means the key is absent; Some(Null) preserves an explicit null.
    pub before: Option<Value>,
    pub after: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct OwnedCompletion {
    previous: Option<Value>,
    env_existed: bool,
    stop_hooks_fingerprint: String,
}

pub(super) fn validate(
    spec: &InstallSpec,
    policy: CompletionBlockingPolicy,
) -> Result<(), IntegrationError> {
    if policy == CompletionBlockingPolicy::NativeDefault {
        return Ok(());
    }
    if spec.agent_key != "claude" {
        return Err(IntegrationError::Unsupported(
            "unbounded completion policy configuration is not implemented for this adapter".into(),
        ));
    }
    if !spec.events.contains(&HookEvent::CompletionProposed) {
        return Err(IntegrationError::Invalid(
            "unbounded completion policy requires a CompletionProposed hook".into(),
        ));
    }
    Ok(())
}

fn stop_hooks(document: &Value) -> String {
    // Preserve no unrelated hook contents in this receipt, only their identity.
    let hooks = match document.pointer("/hooks/Stop") {
        Some(Value::Array(entries)) if entries.is_empty() => &Value::Null,
        Some(hooks) => hooks,
        None => &Value::Null,
    };
    fingerprint(Some(
        &serde_json::to_vec(hooks).expect("JSON Value serializes"),
    ))
}

pub(super) fn install(
    document: &mut Value,
    policy: CompletionBlockingPolicy,
) -> Result<Option<OwnedCompletion>, IntegrationError> {
    if policy == CompletionBlockingPolicy::NativeDefault {
        return Ok(None);
    }
    let stop_hooks_fingerprint = stop_hooks(document);
    let root = document.as_object_mut().expect("validated object");
    let env_existed = root.contains_key("env");
    let env = root
        .entry("env")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            IntegrationError::Invalid(
                "env must be an object to configure completion blocking".into(),
            )
        })?;
    let previous = env.insert(CAP_KEY.into(), json!("0"));
    Ok(Some(OwnedCompletion {
        previous,
        env_existed,
        stop_hooks_fingerprint,
    }))
}

pub(super) fn remove(
    document: &mut Value,
    owned: &OwnedCompletion,
) -> Result<(), IntegrationError> {
    check(document)?;
    // Restoring a limit while a new Stop hook depends on 0 would silently weaken
    // another application's policy. Refuse that ordering, including across SDK
    // state directories. Removing the later borrower first safely retains 0.
    if owned.previous != Some(json!("0")) && stop_hooks(document) != owned.stop_hooks_fingerprint {
        return Err(IntegrationError::Conflict("Stop hooks changed while the unbounded completion setting was owned; remove dependent hooks first or reconcile ownership".into()));
    }
    let env = document
        .get_mut("env")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| {
            IntegrationError::Conflict("owned completion environment is missing".into())
        })?;
    match &owned.previous {
        Some(previous) => {
            env.insert(CAP_KEY.into(), previous.clone());
        }
        None => {
            env.remove(CAP_KEY);
        }
    }
    if env.is_empty() && !owned.env_existed {
        document.as_object_mut().expect("object").remove("env");
    }
    Ok(())
}

pub(super) fn check(document: &Value) -> Result<(), IntegrationError> {
    if document.pointer(CAP_POINTER) != Some(&json!("0")) {
        return Err(IntegrationError::Conflict(
            "owned completion blocking setting was removed or edited".into(),
        ));
    }
    Ok(())
}

pub(super) fn changes(
    before: Option<&[u8]>,
    after: Option<&[u8]>,
) -> Result<Vec<SettingChange>, IntegrationError> {
    let before = document(before)?.pointer(CAP_POINTER).cloned();
    let after = document(after)?.pointer(CAP_POINTER).cloned();
    Ok(if before == after {
        vec![]
    } else {
        vec![SettingChange {
            pointer: CAP_POINTER.into(),
            before,
            after,
        }]
    })
}
