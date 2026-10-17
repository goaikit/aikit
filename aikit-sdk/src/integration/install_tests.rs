use super::*;

#[cfg(unix)]
#[test]
fn owner_drop_releases_installation_lock_with_inherited_description() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("integration.lock");
    let owner = lock(&path).unwrap();
    // A duplicate shares the open description just like inheritance across
    // fork. Keeping it open makes the release failure deterministic.
    let inherited = owner.0.try_clone().unwrap();
    assert!(matches!(lock(&path), Err(IntegrationError::Conflict(_))));
    drop(owner);
    let next_owner = lock(&path).expect("owner drop must explicitly unlock");
    assert!(matches!(lock(&path), Err(IntegrationError::Conflict(_))));
    drop(inherited);
    assert!(matches!(lock(&path), Err(IntegrationError::Conflict(_))));
    drop(next_owner);
    assert!(lock(&path).is_ok());
}

struct Fixture {
    _dir: tempfile::TempDir,
    service: IntegrationService,
    spec: InstallSpec,
    config: PathBuf,
}

#[test]
fn generated_extension_recovers_after_install_and_delete_without_adopting_external_edits() {
    let f = Fixture::new();
    let mut spec = f.spec.clone();
    spec.agent_key = "pi".into();
    spec.events.push(HookEvent::SessionStarted);
    let plan = f.service.plan_install(spec).unwrap();
    assert!(!plan.config_path.exists());
    assert!(matches!(
        f.service.apply_inner(&plan.id, true),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    assert!(plan.config_path.exists());
    let source = fs::read(&plan.config_path).unwrap();
    let reopened = IntegrationService::open(f._dir.path().join("private-state")).unwrap();
    assert!(matches!(
        reopened.apply_install(&plan.id).unwrap(),
        InstallationStatus::Configured { .. }
    ));
    let removal = reopened.plan_remove(&plan.installation_id).unwrap();
    assert!(matches!(
        reopened.apply_inner(&removal.id, true),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    assert!(!plan.config_path.exists());
    fs::write(&plan.config_path, "external replacement").unwrap();
    assert!(matches!(
        reopened.apply_install(&removal.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(
        fs::read_to_string(&plan.config_path).unwrap(),
        "external replacement"
    );
    // Restore the exact pre-removal state, then resume the same operation.
    fs::write(&plan.config_path, source).unwrap();
    assert!(matches!(
        reopened.apply_install(&removal.id).unwrap(),
        InstallationStatus::Absent
    ));
    assert!(matches!(
        reopened.apply_install(&removal.id).unwrap(),
        InstallationStatus::Absent
    ));
    assert!(!plan.config_path.exists());
}

#[test]
fn source_plan_format_upgrade_preserves_v3_json_plans_and_refuses_newer_state() {
    let f = Fixture::new();
    let plan = f.service.plan_install(f.spec.clone()).unwrap();
    let connection = f.service.connection().unwrap();
    connection.execute_batch("UPDATE install_plans SET body=json_remove(body,'$.delete','$.next_receipt.source_fingerprint'); PRAGMA user_version=3;").unwrap();
    drop(connection);
    let reopened = IntegrationService::open(f._dir.path().join("private-state")).unwrap();
    let version: u32 = reopened
        .connection()
        .unwrap()
        .pragma_query_value(None, "user_version", |r| r.get(0))
        .unwrap();
    assert_eq!(version, 5);
    assert!(matches!(
        reopened.apply_install(&plan.id).unwrap(),
        InstallationStatus::Configured { .. }
    ));
    let removal = reopened.plan_remove(&plan.installation_id).unwrap();
    reopened.apply_install(&removal.id).unwrap();
    assert_eq!(f.read(), json!({}));
    reopened
        .connection()
        .unwrap()
        .execute_batch("PRAGMA user_version=6;")
        .unwrap();
    assert!(matches!(
        IntegrationService::open(f._dir.path().join("private-state")),
        Err(IntegrationError::Invalid(_))
    ));
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace with spaces");
        fs::create_dir(&workspace).unwrap();
        let service = IntegrationService::open(dir.path().join("private-state")).unwrap();
        let config = workspace.join(".claude/settings.local.json");
        let spec = InstallSpec {
            application_id: "review-app".into(),
            agent_key: "claude".into(),
            workspace,
            handler: HookCommand {
                executable: std::env::current_exe().unwrap(),
                arguments: vec![
                    "hook".into(),
                    "apostrophe ' dollar $ and semicolon ;".into(),
                ],
            },
            events: vec![HookEvent::BeforeTool, HookEvent::CompletionProposed],
            timeout_seconds: 10,
        };
        Self {
            _dir: dir,
            service,
            spec,
            config,
        }
    }
    fn write(&self, value: &Value) {
        fs::create_dir_all(self.config.parent().unwrap()).unwrap();
        fs::write(&self.config, serde_json::to_vec(value).unwrap()).unwrap();
    }
    fn read(&self) -> Value {
        serde_json::from_slice(&fs::read(&self.config).unwrap()).unwrap()
    }
    fn install(&self) -> InstallPlan {
        let plan = self.service.plan_install(self.spec.clone()).unwrap();
        self.service.apply_install(&plan.id).unwrap();
        plan
    }
}

#[test]
fn install_update_remove_preserve_unrelated_hooks_settings_and_argv() {
    let f = Fixture::new();
    let unrelated = json!({"permissions":{"deny":["Bash(rm *)"]},"env":{"CLAUDE_CODE_STOP_HOOK_BLOCK_CAP":"0"},"hooks":{"Stop":[{"matcher":"", "hooks":[{"type":"command","command":"existing-hook"}]}]}});
    f.write(&unrelated);
    let plan = f.service.plan_install(f.spec.clone()).unwrap();
    assert_eq!(
        f.read(),
        unrelated,
        "preview does not mutate provider configuration"
    );
    let status = f.service.apply_install(&plan.id).unwrap();
    assert!(matches!(status, InstallationStatus::Configured { .. }));
    assert_eq!(
        f.read()["hooks"]["Stop"][1]["hooks"][0]["args"],
        json!(f.spec.handler.arguments)
    );
    assert_eq!(f.read()["hooks"]["Stop"][0], unrelated["hooks"]["Stop"][0]);
    assert_eq!(f.read()["permissions"], unrelated["permissions"]);
    f.service.apply_install(&plan.id).unwrap();
    assert_eq!(f.read()["hooks"]["Stop"].as_array().unwrap().len(), 2);
    let mut updated = f.spec.clone();
    updated.handler.arguments.push("new-argument".into());
    updated.events.push(HookEvent::AfterTool);
    let update = f.service.plan_install(updated).unwrap();
    f.service.apply_install(&update.id).unwrap();
    assert_eq!(f.read()["hooks"]["Stop"].as_array().unwrap().len(), 2);
    let remove = f.service.plan_remove(&plan.installation_id).unwrap();
    assert!(remove.removal);
    assert!(matches!(
        f.service.apply_install(&remove.id).unwrap(),
        InstallationStatus::Absent
    ));
    assert_eq!(f.read(), unrelated);
}

#[test]
fn stale_plan_refuses_to_overwrite_an_external_edit() {
    let f = Fixture::new();
    let plan = f.service.plan_install(f.spec.clone()).unwrap();
    let external = json!({"env":{"OWNER_SETTING":"preserve"}});
    f.write(&external);
    assert!(matches!(
        f.service.apply_install(&plan.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(f.read(), external);
    assert!(matches!(
        f.service
            .installation_status(&plan.installation_id)
            .unwrap(),
        InstallationStatus::Absent
    ));
}

#[test]
fn edited_or_duplicated_owned_entries_are_not_removed() {
    let f = Fixture::new();
    let plan = f.install();
    let original = f.read();
    let mut edited = original.clone();
    edited["hooks"]["Stop"][0]["hooks"][0]["timeout"] = json!(99);
    f.write(&edited);
    assert!(matches!(
        f.service
            .installation_status(&plan.installation_id)
            .unwrap(),
        InstallationStatus::Drifted { .. }
    ));
    assert!(matches!(
        f.service.plan_remove(&plan.installation_id),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(matches!(
        f.service.plan_install(f.spec.clone()),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(f.read(), edited);
    let mut duplicate = original;
    let entry = duplicate["hooks"]["Stop"][0].clone();
    duplicate["hooks"]["Stop"]
        .as_array_mut()
        .unwrap()
        .push(entry);
    f.write(&duplicate);
    assert!(f.service.plan_remove(&plan.installation_id).is_err());
    assert_eq!(f.read(), duplicate);
}

#[test]
fn crash_after_config_write_resumes_without_duplicate_installation() {
    let f = Fixture::new();
    let plan = f.service.plan_install(f.spec.clone()).unwrap();
    assert!(matches!(
        f.service.apply_inner(&plan.id, true),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    let written = fs::read(&f.config).unwrap();
    let recovered = IntegrationService::open(&f.service.state).unwrap();
    assert!(matches!(
        recovered
            .installation_status(&plan.installation_id)
            .unwrap(),
        InstallationStatus::RecoveryRequired { .. }
    ));
    assert!(recovered.plan_install(f.spec.clone()).is_err());
    let mut other = f.spec.clone();
    other.application_id = "another-app".into();
    assert!(matches!(
        recovered.plan_install(other),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    assert!(matches!(
        recovered.apply_install(&plan.id).unwrap(),
        InstallationStatus::Configured { .. }
    ));
    assert_eq!(fs::read(&f.config).unwrap(), written);
}

#[test]
fn interrupted_removal_is_recoverable_and_does_not_remove_new_external_content() {
    let f = Fixture::new();
    let plan = f.install();
    let remove = f.service.plan_remove(&plan.installation_id).unwrap();
    assert!(f.service.apply_inner(&remove.id, true).is_err());
    let recovered = IntegrationService::open(&f.service.state).unwrap();
    assert!(matches!(
        recovered.apply_install(&remove.id).unwrap(),
        InstallationStatus::Absent
    ));
    assert_eq!(f.read(), json!({}));
    let plan = f.install();
    let remove = f.service.plan_remove(&plan.installation_id).unwrap();
    assert!(f.service.apply_inner(&remove.id, true).is_err());
    let external = json!({"hooks":{"Stop":[{"hooks":[{"type":"command","command":"owner"}]}]}});
    f.write(&external);
    assert!(matches!(
        recovered.apply_install(&remove.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(f.read(), external);
}

#[test]
fn config_drift_after_prepared_installation_is_not_silently_adopted() {
    let f = Fixture::new();
    let plan = f.service.plan_install(f.spec.clone()).unwrap();
    assert!(f.service.apply_inner(&plan.id, true).is_err());
    let mut external = f.read();
    external["extra"] = json!("owner changed this");
    f.write(&external);
    assert!(matches!(
        f.service.apply_install(&plan.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(f.read(), external);
}

#[test]
fn unrelated_changes_after_install_are_preserved_on_removal() {
    let f = Fixture::new();
    let plan = f.install();
    let mut external = f.read();
    external["extra"] = json!("owner setting");
    f.write(&external);
    assert!(matches!(
        f.service
            .installation_status(&plan.installation_id)
            .unwrap(),
        InstallationStatus::Configured { .. }
    ));
    let remove = f.service.plan_remove(&plan.installation_id).unwrap();
    f.service.apply_install(&remove.id).unwrap();
    assert_eq!(f.read(), json!({"extra":"owner setting"}));
}

#[test]
fn malformed_disabled_and_non_object_configs_are_rejected_without_replacement() {
    let f = Fixture::new();
    fs::create_dir_all(f.config.parent().unwrap()).unwrap();
    for raw in [
        "not json",
        "[]",
        r#"{"hooks":[]}"#,
        r#"{"hooks":{"Stop":{}}}"#,
        r#"{"disableAllHooks":true}"#,
    ] {
        fs::write(&f.config, raw).unwrap();
        assert!(f.service.plan_install(f.spec.clone()).is_err());
        assert_eq!(fs::read_to_string(&f.config).unwrap(), raw);
    }
}

#[test]
fn unsupported_provider_unknown_catalog_key_and_expanding_arguments_are_explicit() {
    let f = Fixture::new();
    for key in ["cursor", "gemini"] {
        let mut spec = f.spec.clone();
        spec.agent_key = key.into();
        assert!(matches!(
            f.service.plan_install(spec),
            Err(IntegrationError::Unsupported(_))
        ));
    }
    let mut spec = f.spec.clone();
    spec.agent_key = "made-up-provider".into();
    assert!(matches!(
        f.service.plan_install(spec),
        Err(IntegrationError::UnknownAgent(_))
    ));
    let mut spec = f.spec.clone();
    spec.handler.arguments = vec!["${CLAUDE_PROJECT_DIR}".into()];
    assert!(matches!(
        f.service.plan_install(spec),
        Err(IntegrationError::Invalid(_))
    ));
    assert!(!f.config.exists());
}

#[test]
fn different_apps_cannot_claim_identical_existing_entries() {
    let f = Fixture::new();
    f.install();
    let mut spec = f.spec.clone();
    spec.application_id = "other-owner".into();
    assert!(matches!(
        f.service.plan_install(spec),
        Err(IntegrationError::Conflict(_))
    ));
}

#[test]
fn schema_upgrade_preserves_receipts_and_completed_plans_discard_config_bodies() {
    let f = Fixture::new();
    f.write(&json!({"env":{"UNRELATED_PRIVATE_VALUE":"sensitive-fixture"}}));
    let plan = f.install();
    let connection = f.service.connection().unwrap();
    let body: String = connection
        .query_row(
            "SELECT body FROM install_plans WHERE id=?1",
            [&plan.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(!body.contains("sensitive-fixture"));
    let pending: Pending = serde_json::from_str(&body).unwrap();
    assert!(pending.after.is_empty());
    assert!(pending.previous_receipt.is_none());
    assert!(pending.next_receipt.is_none());
    // Model the previous schema, which has install records but no hook journal.
    connection
        .execute_batch("DROP TABLE hook_invocations; DROP TABLE session_bindings; DROP TABLE installation_revisions; PRAGMA user_version=1;")
        .unwrap();
    drop(connection);
    let upgraded = IntegrationService::open(&f.service.state).unwrap();
    assert!(matches!(
        upgraded.installation_status(&plan.installation_id).unwrap(),
        InstallationStatus::Configured { .. }
    ));
    assert!(upgraded
        .events(&plan.installation_id, 0, 10)
        .unwrap()
        .records
        .is_empty());
    assert_eq!(
        upgraded
            .find_installation("review-app", "claude", &f.spec.workspace)
            .unwrap()
            .id,
        plan.installation_id
    );
    assert!(upgraded
        .find_installation("another-app", "claude", &f.spec.workspace)
        .is_err());
    assert_eq!(
        f.read()["env"]["UNRELATED_PRIVATE_VALUE"],
        "sensitive-fixture"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_config_directory_cannot_redirect_installation() {
    let f = Fixture::new();
    let target = f._dir.path().join("outside");
    fs::create_dir(&target).unwrap();
    std::os::unix::fs::symlink(&target, f.config.parent().unwrap()).unwrap();
    assert!(matches!(
        f.service.plan_install(f.spec.clone()),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(fs::read_dir(target).unwrap().next().is_none());
}
