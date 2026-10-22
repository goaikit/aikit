//! Generated tool files use the existing installation receipt and plan journal.
use super::*;

impl IntegrationService {
    /// Add/update a generated tool on an existing configured hook installation.
    /// This plans project-scope files only; no native process or profile is used.
    /// Configured files do not establish effective loading or caller authority.
    pub fn plan_agent_tool(
        &self,
        installation_id: &str,
        spec: super::super::AgentToolSpec,
    ) -> Result<InstallPlan, IntegrationError> {
        self.plan_agent_tool_inner(installation_id, spec, None)
    }

    /// Opt-in Claude handoff candidate for qualification, using a fixed stdio
    /// handler. This does not qualify native admission, acknowledgement or input
    /// consumption, and does not enable readiness. Default tools have no pump.
    pub fn plan_agent_tool_with_message_bridge(
        &self,
        installation_id: &str,
        spec: super::super::AgentToolSpec,
        message_handler: HookCommand,
    ) -> Result<InstallPlan, IntegrationError> {
        self.plan_agent_tool_inner(installation_id, spec, Some(&message_handler))
    }

    fn plan_agent_tool_inner(
        &self,
        installation_id: &str,
        spec: super::super::AgentToolSpec,
        message_handler: Option<&HookCommand>,
    ) -> Result<InstallPlan, IntegrationError> {
        let _lock = self.state_lock()?;
        let connection = self.connection()?;
        reject_pending(&connection, installation_id)?;
        let raw = receipt_json(&connection, installation_id)?.ok_or(IntegrationError::NotFound)?;
        let mut receipt: Receipt = serde_json::from_str(&raw)?;
        let installation = receipt.installation.clone();
        if spec.native_completion_bridge {
            require_detached_bindings(&connection, installation_id)?;
        }
        super::super::command::validate_handler(&installation.spec.handler)?;
        if spec.application_id != installation.spec.application_id
            || spec.installation_id != installation.id
        {
            return Err(IntegrationError::Invalid(
                "tool identity must match its installation".into(),
            ));
        }
        reject_prepared_config(&connection, &installation.config_path, None)?;
        if !matches!(
            self.status_inner(&connection, installation_id)?,
            InstallationStatus::Configured { .. }
        ) {
            return Err(IntegrationError::Conflict(
                "hook/tool installation has drifted".into(),
            ));
        }
        if message_handler.is_some()
            && !installation
                .spec
                .events
                .contains(&HookEvent::InputSubmitted)
        {
            return Err(IntegrationError::Invalid(
                "message bridge requires input observations".into(),
            ));
        }
        super::super::agent_tools::validate_completion_events(&spec, &installation.spec.events)?;
        let plugin = super::super::agent_tools::render_with_message_handler(
            &installation.spec.agent_key,
            &spec,
            Some(&installation.spec.handler),
            message_handler,
        )?;
        receipt.agent_tool = Some(owned_agent_tool(&plugin));
        let before = read_config(&installation.spec.workspace, &installation.config_path)?;
        self.save_bytes_plan_with_tool(
            &connection,
            before.clone(),
            before,
            Some(raw),
            Some(receipt),
            &installation,
            Some(&plugin),
            spec.native_completion_bridge,
            None,
        )
    }
}

pub(super) fn owned_agent_tool(plugin: &super::super::AgentToolPlugin) -> OwnedAgentTool {
    OwnedAgentTool {
        invocation_env_key: Some(plugin.invocation_environment_key().into()),
        relative_root: PathBuf::from(".claude").join("skills").join(plugin.name()),
        files: plugin
            .files()
            .iter()
            .map(|(name, bytes)| (name.clone(), fingerprint(Some(bytes.as_bytes()))))
            .collect(),
    }
}

pub(super) fn check_agent_tool(
    workspace: &Path,
    tool: &OwnedAgentTool,
) -> Result<(), IntegrationError> {
    for (name, expected) in &tool.files {
        let bytes = read_config(workspace, &workspace.join(&tool.relative_root).join(name))?;
        if fingerprint(bytes.as_deref()) != *expected {
            return Err(IntegrationError::Conflict(
                "owned tool file was removed or edited".into(),
            ));
        }
    }
    Ok(())
}

pub(super) fn tool_file_changes(
    workspace: &Path,
    previous: Option<&str>,
    next: Option<&Receipt>,
    plugin: Option<&super::super::AgentToolPlugin>,
) -> Result<Vec<PendingToolFile>, IntegrationError> {
    let previous = previous.map(serde_json::from_str::<Receipt>).transpose()?;
    let old = previous
        .as_ref()
        .and_then(|receipt| receipt.agent_tool.as_ref());
    let new = next.and_then(|receipt| receipt.agent_tool.as_ref());
    if let Some(tool) = old {
        check_agent_tool(workspace, tool)?;
    }
    if let Some(tool) = new {
        if old.is_none() {
            let root = workspace.join(&tool.relative_root);
            check_config_path(workspace, &root)?;
            if root.try_exists()? {
                return Err(IntegrationError::Conflict(
                    "unowned tool directory already exists".into(),
                ));
            }
        }
        if let Some(old) = old {
            if old.relative_root != tool.relative_root {
                return Err(IntegrationError::Conflict(
                    "owned tool identity cannot be replaced".into(),
                ));
            }
        }
    }
    let Some(root) = new.or(old).map(|tool| workspace.join(&tool.relative_root)) else {
        return Ok(Vec::new());
    };
    let names: BTreeSet<_> = old
        .into_iter()
        .chain(new)
        .flat_map(|tool| tool.files.keys())
        .collect();
    let mut changes = Vec::new();
    for name in names {
        let before = old
            .and_then(|tool| tool.files.get(name))
            .cloned()
            .unwrap_or_else(|| "absent".into());
        let after_hash = new.and_then(|tool| tool.files.get(name));
        if after_hash == Some(&before) {
            continue;
        }
        let after = after_hash
            .map(|_| {
                plugin
                    .and_then(|plugin| plugin.files().get(name))
                    .map(|bytes| bytes.as_bytes().to_vec())
                    .ok_or_else(|| {
                        IntegrationError::Invalid("missing generated tool contents".into())
                    })
            })
            .transpose()?;
        let path = root.join(name);
        if fingerprint(read_config(workspace, &path)?.as_deref()) != before {
            return Err(IntegrationError::Conflict(
                "unowned or edited tool file".into(),
            ));
        }
        changes.push(PendingToolFile {
            path,
            before,
            after,
        });
    }
    // Manifest last for installation/update; first for removal. No folder-wide
    // replacement or recursive deletion: unrelated files remain untouched.
    changes.sort_by_key(|file| {
        let manifest = file.path.ends_with(".claude-plugin/plugin.json");
        if new.is_some() {
            manifest
        } else {
            !manifest
        }
    });
    Ok(changes)
}

pub(super) fn prune_removed_tool_directories(
    workspace: &Path,
    previous: Option<&str>,
    next: Option<&Receipt>,
) -> Result<(), IntegrationError> {
    if next
        .and_then(|receipt| receipt.agent_tool.as_ref())
        .is_some()
    {
        return Ok(());
    }
    let previous = previous.map(serde_json::from_str::<Receipt>).transpose()?;
    let Some(tool) = previous.and_then(|receipt| receipt.agent_tool) else {
        return Ok(());
    };
    let root = workspace.join(tool.relative_root);
    let mut directories = BTreeSet::from([root.clone()]);
    for name in tool.files.keys() {
        let path = root.join(name);
        let mut parent = path.parent();
        while let Some(directory) = parent.filter(|directory| directory.starts_with(&root)) {
            directories.insert(directory.to_owned());
            parent = directory.parent();
        }
    }
    let mut directories: Vec<_> = directories.into_iter().collect();
    directories.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
    for directory in directories {
        check_config_path(workspace, &directory)?;
        match fs::remove_dir(&directory) {
            Ok(()) => {
                #[cfg(unix)]
                fs::File::open(directory.parent().expect("owned directory parent"))?.sync_all()?;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}
