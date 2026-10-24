//! Inherited generation claims from the owned native module, not credentials or
//! native-process authentication. The installation selects the environment key;
//! native JSON cannot override it. Reuse existing journal/binding validation.
use super::{HookRequest, Installation, IntegrationError, NativeToolContext};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SessionEnvironment {
    schema_version: u32,
    installation_id: String,
    native_context: NativeToolContext,
    #[serde(default)]
    native_turn_id: Option<String>,
}

/// Compare inherited module claims with a separate envelope. This is correlation
/// only, not authentication of a physical native caller or native admission.
pub(super) fn validate_tool_environment(
    installation: &Installation,
    native: &NativeToolContext,
    value: Option<&str>,
) -> Result<(), IntegrationError> {
    let refused =
        || IntegrationError::Invalid("owned message context is missing or inconsistent".into());
    let context: SessionEnvironment =
        serde_json::from_str(value.filter(|v| v.len() <= 64 * 1024).ok_or_else(refused)?)
            .map_err(|_| refused())?;
    if context.schema_version != 1
        || context.installation_id != installation.id
        || context.native_context.session_id != native.session_id
        || context.native_context.invocation_id != native.invocation_id
        || !context.native_context.cwd.is_absolute()
        || std::fs::canonicalize(&native.cwd).map_err(|_| refused())? != installation.spec.workspace
        || std::fs::canonicalize(context.native_context.cwd).map_err(|_| refused())?
            != installation.spec.workspace
    {
        return Err(refused());
    }
    Ok(())
}

pub(super) fn bind_environment(
    installation: &Installation,
    request: &mut HookRequest,
    value: Option<&str>,
) -> Result<(), IntegrationError> {
    let refused = || {
        IntegrationError::Invalid(
            "owned native invocation context is missing or inconsistent".into(),
        )
    };
    let value = value
        .filter(|value| value.len() <= 64 * 1024)
        .ok_or_else(refused)?;
    let context: SessionEnvironment = serde_json::from_str(value).map_err(|_| refused())?;
    if installation.spec.agent_key != "claude"
        || context.schema_version != 1
        || context.installation_id != installation.id
        || context.native_context.session_id != request.session_id
        || uuid::Uuid::parse_str(&context.native_context.invocation_id).is_err()
        || !context.native_context.cwd.is_absolute()
        || std::fs::canonicalize(context.native_context.cwd).map_err(|_| refused())?
            != installation.spec.workspace
    {
        return Err(refused());
    }
    request.invocation_id = Some(context.native_context.invocation_id);
    if let Some(turn) = &request.native_turn {
        if Some(&turn.id) != context.native_turn_id.as_ref() {
            return Err(refused());
        }
    } else if let Some(id) = context.native_turn_id {
        if id.trim().is_empty() || id.len() > 256 || id.contains('\0') {
            return Err(refused());
        }
        request.native_turn = Some(super::NativeTurn { id, outcome: None });
    }
    Ok(())
}
