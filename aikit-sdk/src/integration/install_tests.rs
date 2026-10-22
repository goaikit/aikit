use super::*;

#[test]
fn malformed_legacy_detachment_scope_refuses_upgrade_without_replacing_history() {
    let f = Fixture::new();
    f.install();
    let config = fs::read(&f.config).unwrap();
    let connection = f.service.connection().unwrap();
    connection.execute_batch("INSERT INTO session_bindings VALUES ('bad-legacy','{}',1); DROP TABLE binding_admission_floors; PRAGMA user_version=14").unwrap();
    assert!(IntegrationService::upgrade_existing(&f.service.state).is_err());
    assert_eq!(
        connection
            .pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))
            .unwrap(),
        14
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT reference FROM session_bindings WHERE id='bad-legacy'",
                [],
                |r| r.get(0)
            )
            .unwrap(),
        "{}"
    );
    assert_eq!(fs::read(&f.config).unwrap(), config);
}

#[test]
fn schema_fourteen_upgrade_seeds_detached_binding_admission_floor() {
    let f = Fixture::new();
    let hooks = f.install();
    synthetic_binding(&f, &hooks.installation_id);
    f.service.connection().unwrap().execute_batch("UPDATE session_bindings SET detached=1; DROP TABLE binding_admission_floors; PRAGMA user_version=14").unwrap();
    assert!(IntegrationService::open_existing(&f.service.state).is_err());
    let report = IntegrationService::upgrade_existing(&f.service.state).unwrap();
    assert_eq!(
        (
            report.previous_schema,
            report.current_schema,
            report.changed
        ),
        (14, 15, true)
    );
    let reopened = IntegrationService::open_existing(&f.service.state).unwrap();
    let floor: i64 = reopened
        .connection()
        .unwrap()
        .query_row(
            "SELECT cursor FROM binding_admission_floors WHERE installation_id=?1",
            [&hooks.installation_id],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(floor, 0);
    assert!(reopened
        .attached_bindings("review-app", "claude", &f.spec.workspace, None, 50)
        .unwrap()
        .records
        .is_empty());
}

#[test]
fn schema_thirteen_upgrade_retains_caller_update_identity_and_refuses_unsettled_work() {
    let f = Fixture::new();
    let hooks = f.install();
    let tool = tool_spec(&f, &hooks.installation_id);
    f.service
        .plan_update_with_agent_tool_id(
            "upgrade-intent",
            f.spec.clone(),
            CompletionBlockingPolicy::NativeDefault,
            tool.clone(),
            None,
        )
        .unwrap();
    f.service
        .connection()
        .unwrap()
        .pragma_update(None, "user_version", 13)
        .unwrap();
    assert!(IntegrationService::open_existing(&f.service.state).is_err());
    assert!(matches!(
        IntegrationService::upgrade_existing(&f.service.state),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    f.service.apply_install("upgrade-intent").unwrap();
    let report = IntegrationService::upgrade_existing(&f.service.state).unwrap();
    assert_eq!(
        (
            report.previous_schema,
            report.current_schema,
            report.changed
        ),
        (13, 15, true)
    );
    let reopened = IntegrationService::open_existing(&f.service.state).unwrap();
    assert_eq!(
        reopened
            .plan_update_with_agent_tool_id(
                "upgrade-intent",
                f.spec.clone(),
                CompletionBlockingPolicy::NativeDefault,
                tool,
                None
            )
            .unwrap()
            .id,
        "upgrade-intent"
    );
    reopened
        .applied_installation_guard("upgrade-intent")
        .unwrap();
}

#[test]
fn consumer_update_id_survives_lost_plan_reply_and_guard_refuses_superseded_revision() {
    let f = Fixture::new();
    let hooks = f.install();
    let mut spec = f.spec.clone();
    spec.handler.arguments.push("new-startup".into());
    let tool = tool_spec(&f, &hooks.installation_id);
    let plan = f
        .service
        .plan_update_with_agent_tool_id(
            "owner-update",
            spec.clone(),
            CompletionBlockingPolicy::NativeDefault,
            tool.clone(),
            None,
        )
        .unwrap();
    assert_eq!(plan.id, "owner-update");
    assert_eq!(
        f.service
            .plan_update_with_agent_tool_id(
                "owner-update",
                spec.clone(),
                CompletionBlockingPolicy::NativeDefault,
                tool.clone(),
                None
            )
            .unwrap()
            .id,
        plan.id
    );
    let mut changed = tool.clone();
    changed.instructions.push_str(" changed");
    assert!(matches!(
        f.service.plan_update_with_agent_tool_id(
            "owner-update",
            spec.clone(),
            CompletionBlockingPolicy::NativeDefault,
            changed,
            None
        ),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(f.service.applied_installation_guard(&plan.id).is_err());
    assert!(f
        .service
        .apply_with_tool_fault(&plan.id, false, Some(1))
        .is_err());
    assert_eq!(
        f.service
            .plan_update_with_agent_tool_id(
                "owner-update",
                spec.clone(),
                CompletionBlockingPolicy::NativeDefault,
                tool.clone(),
                None
            )
            .unwrap()
            .id,
        plan.id
    );
    let reopened = IntegrationService::open_existing(&f.service.state).unwrap();
    reopened.apply_install(&plan.id).unwrap();
    let guard = reopened.applied_installation_guard(&plan.id).unwrap();
    assert_eq!(guard.installation().spec, spec);
    assert!(reopened.plan_remove(&hooks.installation_id).is_err());
    drop(guard);
    let next = reopened
        .plan_agent_tool(&hooks.installation_id, tool.clone())
        .unwrap();
    reopened.apply_install(&next.id).unwrap();
    assert!(reopened.applied_installation_guard(&plan.id).is_err());
    assert_eq!(
        reopened
            .plan_update_with_agent_tool_id(
                "owner-update",
                spec,
                CompletionBlockingPolicy::NativeDefault,
                tool,
                None
            )
            .unwrap()
            .id,
        plan.id
    );
    assert_eq!(
        reopened
            .connection()
            .unwrap()
            .query_row(
                "SELECT count(*) FROM install_plans WHERE id='owner-update'",
                [],
                |r| r.get::<_, u32>(0)
            )
            .unwrap(),
        1
    );
}

#[test]
fn consumer_update_cancel_removes_only_an_unstarted_owned_preview() {
    let f = Fixture::new();
    let hooks = f.install();
    let tool = tool_spec(&f, &hooks.installation_id);
    let bytes = fs::read(&f.config).unwrap();
    f.service
        .plan_update_with_agent_tool_id(
            "cancel-me",
            f.spec.clone(),
            CompletionBlockingPolicy::NativeDefault,
            tool.clone(),
            None,
        )
        .unwrap();
    assert!(f
        .service
        .discard_planned_update("cancel-me", "foreign", "claude", &f.spec.workspace)
        .is_err());
    f.service
        .discard_planned_update("cancel-me", "review-app", "claude", &f.spec.workspace)
        .unwrap();
    assert!(matches!(
        f.service.apply_install("cancel-me"),
        Err(IntegrationError::NotFound)
    ));
    assert_eq!(fs::read(&f.config).unwrap(), bytes);
    f.service
        .discard_planned_update("absent", "review-app", "claude", &f.spec.workspace)
        .unwrap();
    f.service
        .plan_update_with_agent_tool_id(
            "begun",
            f.spec.clone(),
            CompletionBlockingPolicy::NativeDefault,
            tool,
            None,
        )
        .unwrap();
    assert!(f.service.apply_inner("begun", true).is_err());
    assert!(f
        .service
        .discard_planned_update("begun", "review-app", "claude", &f.spec.workspace)
        .is_err());
    f.service.apply_install("begun").unwrap();
    assert!(f
        .service
        .discard_planned_update("begun", "review-app", "claude", &f.spec.workspace)
        .is_err());
}

fn replacement_plan(f: &Fixture, id: &str) -> Result<InstallPlan, IntegrationError> {
    let mut spec = f.spec.clone();
    spec.handler.arguments.push("replacement-startup".into());
    let mut tool = tool_spec(f, id);
    tool.handler.arguments.push("replacement-callable".into());
    tool.instructions.push_str(" replacement instructions");
    f.service
        .plan_update_with_agent_tool(spec, CompletionBlockingPolicy::NativeDefault, tool, None)
}

fn synthetic_binding(f: &Fixture, id: &str) {
    let reference = super::super::SessionRef {
        installation_id: id.into(),
        installation_revision: "fixture".into(),
        native_session_id: "fixture".into(),
        invocation_id: None,
        start_cursor: 1,
    };
    f.service
        .connection()
        .unwrap()
        .execute(
            "INSERT INTO session_bindings VALUES ('bundle-fixture',?1,0)",
            [serde_json::to_string(&reference).unwrap()],
        )
        .unwrap();
}

#[test]
fn bundled_replacement_recovers_partial_files_and_publishes_one_consistent_receipt() {
    let f = Fixture::new();
    f.write(&json!({"owner":"retain"}));
    let hooks = f.install();
    let tool = f
        .service
        .plan_agent_tool(
            &hooks.installation_id,
            tool_spec(&f, &hooks.installation_id),
        )
        .unwrap();
    f.service.apply_install(&tool.id).unwrap();
    let before = fs::read(&f.config).unwrap();
    let plan = replacement_plan(&f, &hooks.installation_id).unwrap();
    assert!(!plan.removal && !plan.files.is_empty());
    assert_eq!(fs::read(&f.config).unwrap(), before);
    assert!(matches!(
        f.service.apply_with_tool_fault(&plan.id, false, Some(1)),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    assert_eq!(fs::read(&f.config).unwrap(), before);
    let reopened = IntegrationService::open_existing(&f.service.state).unwrap();
    assert_eq!(
        reopened
            .pending_installation_plan("review-app", "claude", &f.spec.workspace)
            .unwrap()
            .unwrap()
            .id,
        plan.id
    );
    let InstallationStatus::Configured { installation } = reopened.apply_install(&plan.id).unwrap()
    else {
        panic!("not configured")
    };
    assert!(installation
        .spec
        .handler
        .arguments
        .contains(&"replacement-startup".into()));
    assert_eq!(f.read()["owner"], "retain");
    let raw = receipt_json(&reopened.connection().unwrap(), &hooks.installation_id)
        .unwrap()
        .unwrap();
    let receipt: Receipt = serde_json::from_str(&raw).unwrap();
    check_agent_tool(&f.spec.workspace, receipt.agent_tool.as_ref().unwrap()).unwrap();
    let root = f
        .spec
        .workspace
        .join(&receipt.agent_tool.unwrap().relative_root);
    assert!(fs::read_to_string(root.join("hooks/register.js"))
        .unwrap()
        .contains("replacement-startup"));
    assert!(fs::read_to_string(root.join("hooks/register.js"))
        .unwrap()
        .contains("replacement-callable"));
    reopened.apply_install(&plan.id).unwrap();
}

#[test]
fn bundled_replacement_rechecks_detach_at_plan_apply_and_recovery() {
    let f = Fixture::new();
    let hooks = f.install();
    synthetic_binding(&f, &hooks.installation_id);
    let before = fs::read(&f.config).unwrap();
    assert!(replacement_plan(&f, &hooks.installation_id).is_err());
    f.service
        .connection()
        .unwrap()
        .execute("UPDATE session_bindings SET detached=1", [])
        .unwrap();
    let plan = replacement_plan(&f, &hooks.installation_id).unwrap();
    f.service
        .connection()
        .unwrap()
        .execute("UPDATE session_bindings SET detached=0", [])
        .unwrap();
    assert!(f.service.apply_install(&plan.id).is_err());
    assert_eq!(fs::read(&f.config).unwrap(), before);
    f.service
        .connection()
        .unwrap()
        .execute("UPDATE session_bindings SET detached=1", [])
        .unwrap();
    assert!(f
        .service
        .apply_with_tool_fault(&plan.id, false, Some(1))
        .is_err());
    f.service
        .connection()
        .unwrap()
        .execute("UPDATE session_bindings SET detached=0", [])
        .unwrap();
    assert!(f.service.apply_install(&plan.id).is_err());
    f.service
        .connection()
        .unwrap()
        .execute("UPDATE session_bindings SET detached=1", [])
        .unwrap();
    f.service.apply_install(&plan.id).unwrap();
}

#[test]
fn schema_twelve_upgrade_requires_detached_settled_state_and_retains_receipts() {
    let f = Fixture::new();
    let installed = f.install();
    let bytes = fs::read(&f.config).unwrap();
    synthetic_binding(&f, &installed.installation_id);
    f.service
        .connection()
        .unwrap()
        .pragma_update(None, "user_version", 12)
        .unwrap();
    assert!(IntegrationService::open_existing(&f.service.state).is_err());
    assert!(IntegrationService::upgrade_existing(&f.service.state).is_err());
    f.service
        .connection()
        .unwrap()
        .execute("UPDATE session_bindings SET detached=1", [])
        .unwrap();
    f.service
        .connection()
        .unwrap()
        .execute(
            "INSERT INTO install_plans VALUES ('unsettled','fixture','planned','{}')",
            [],
        )
        .unwrap();
    assert!(matches!(
        IntegrationService::upgrade_existing(&f.service.state),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    f.service
        .connection()
        .unwrap()
        .execute("DELETE FROM install_plans WHERE id='unsettled'", [])
        .unwrap();
    let upgrade = IntegrationService::upgrade_existing(&f.service.state).unwrap();
    assert_eq!(
        (
            upgrade.previous_schema,
            upgrade.current_schema,
            upgrade.changed
        ),
        (12, 15, true)
    );
    assert_eq!(fs::read(&f.config).unwrap(), bytes);
    let reopened = IntegrationService::open_existing(&f.service.state).unwrap();
    assert_eq!(
        reopened
            .find_installation("review-app", "claude", &f.spec.workspace)
            .unwrap()
            .id,
        installed.installation_id
    );
    assert!(
        !IntegrationService::upgrade_existing(&f.service.state)
            .unwrap()
            .changed
    );
}

#[test]
fn pending_plan_lookup_recovers_only_the_scoped_interrupted_apply() {
    let f = Fixture::new();
    let original = json!({"owner":true});
    f.write(&original);
    let hooks = f.install();
    let removal = f.service.plan_remove(&hooks.installation_id).unwrap();
    assert!(f
        .service
        .pending_installation_plan("review-app", "claude", &f.spec.workspace)
        .unwrap()
        .is_none());
    assert!(matches!(
        f.service.apply_inner(&removal.id, true),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    let reopened = IntegrationService::open_existing(&f.service.state).unwrap();
    let pending = reopened
        .pending_installation_plan("review-app", "claude", &f.spec.workspace)
        .unwrap()
        .unwrap();
    assert_eq!(pending.id, removal.id);
    assert!(pending.removal);
    assert!(reopened
        .pending_installation_plan("other-app", "claude", &f.spec.workspace)
        .unwrap()
        .is_none());
    assert!(reopened
        .pending_installation_plan("review-app", "codex", &f.spec.workspace)
        .unwrap()
        .is_none());
    let other = f._dir.path().join("other");
    fs::create_dir(&other).unwrap();
    assert!(reopened
        .pending_installation_plan("review-app", "claude", &other)
        .unwrap()
        .is_none());
    assert!(matches!(
        reopened.apply_install(&pending.id).unwrap(),
        InstallationStatus::Absent
    ));
    assert_eq!(f.read(), original);
    assert!(reopened
        .pending_installation_plan("review-app", "claude", &f.spec.workspace)
        .unwrap()
        .is_none());
}

#[test]
fn pending_plan_lookup_exposes_install_kind_without_applying_and_keeps_conflicts() {
    let f = Fixture::new();
    let install = f.service.plan_install(f.spec.clone()).unwrap();
    assert!(f.service.apply_inner(&install.id, true).is_err());
    let before = fs::read(&f.config).unwrap();
    let pending = f
        .service
        .pending_installation_plan("review-app", "claude", &f.spec.workspace)
        .unwrap()
        .unwrap();
    assert!(!pending.removal);
    assert_eq!(fs::read(&f.config).unwrap(), before);
    f.write(&json!({"external":"preserve"}));
    assert!(matches!(
        f.service.apply_install(&pending.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(f.read(), json!({"external":"preserve"}));
    assert!(f
        .service
        .pending_installation_plan("review-app", "claude", &f.spec.workspace)
        .unwrap()
        .is_some());
}

#[test]
fn pending_plan_lookup_rejects_corrupt_scope() {
    let f = Fixture::new();
    let install = f.service.plan_install(f.spec.clone()).unwrap();
    assert!(f.service.apply_inner(&install.id, true).is_err());
    let connection = f.service.connection().unwrap();
    let raw: String = connection
        .query_row(
            "SELECT body FROM install_plans WHERE id=?1",
            [&install.id],
            |r| r.get(0),
        )
        .unwrap();
    let mut pending: Pending = serde_json::from_str(&raw).unwrap();
    pending.preview.installation_id = "foreign".into();
    connection
        .execute(
            "UPDATE install_plans SET body=?2 WHERE id=?1",
            params![install.id, serde_json::to_string(&pending).unwrap()],
        )
        .unwrap();
    assert!(matches!(
        f.service
            .pending_installation_plan("review-app", "claude", &f.spec.workspace),
        Err(IntegrationError::Conflict(_))
    ));
}

#[test]
fn existing_open_never_creates_a_missing_or_empty_journal() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state");
    assert!(matches!(
        IntegrationService::open_existing(&state),
        Err(IntegrationError::NotFound)
    ));
    assert!(!state.exists());
    let created = IntegrationService::open(&state).unwrap();
    let database = state.join("integration.db");
    fs::remove_file(&database).unwrap();
    assert!(matches!(
        IntegrationService::open_existing(&state),
        Err(IntegrationError::NotFound)
    ));
    assert!(!database.exists());
    fs::write(&database, []).unwrap();
    assert!(matches!(
        IntegrationService::open_existing(&state),
        Err(IntegrationError::Invalid(_))
    ));
    assert_eq!(fs::metadata(&database).unwrap().len(), 0);
    drop(created);
}

#[test]
fn existing_handle_cannot_recreate_a_journal_removed_after_open() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state");
    IntegrationService::open(&state).unwrap();
    let existing = IntegrationService::open_existing(&state).unwrap();
    let database = state.join("integration.db");
    fs::remove_file(&database).unwrap();
    assert!(existing.connection().is_err());
    assert!(!database.exists());
}

#[test]
fn existing_open_requires_explicit_upgrade_without_losing_owned_receipts() {
    let fixture = Fixture::new();
    let installed = fixture.install();
    fixture
        .service
        .connection()
        .unwrap()
        .pragma_update(None, "user_version", 9)
        .unwrap();
    assert!(matches!(
        IntegrationService::open_existing(&fixture.service.state),
        Err(IntegrationError::Invalid(_))
    ));
    assert_eq!(
        fixture
            .service
            .connection()
            .unwrap()
            .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        9
    );
    // Only the separate owner-controlled initializer performs an upgrade.
    IntegrationService::open(&fixture.service.state).unwrap();
    let existing = IntegrationService::open_existing(&fixture.service.state).unwrap();
    assert_eq!(
        existing.installed(&installed.installation_id).unwrap().id,
        installed.installation_id
    );
    assert_eq!(
        existing
            .connection()
            .unwrap()
            .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        15
    );
}

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

#[test]
fn explicit_upgrade_never_initializes_missing_empty_or_unsupported_state() {
    let directory = tempfile::tempdir().unwrap();
    let state = directory.path().join("state");
    assert!(matches!(
        IntegrationService::upgrade_existing(&state),
        Err(IntegrationError::NotFound)
    ));
    assert!(!state.exists());
    IntegrationService::open(&state).unwrap();
    let database = state.join("integration.db");
    fs::remove_file(&database).unwrap();
    assert!(matches!(
        IntegrationService::upgrade_existing(&state),
        Err(IntegrationError::NotFound)
    ));
    assert!(!database.exists());
    fs::write(&database, []).unwrap();
    assert!(matches!(
        IntegrationService::upgrade_existing(&state),
        Err(IntegrationError::Invalid(_))
    ));
    assert_eq!(fs::metadata(&database).unwrap().len(), 0);
    fs::remove_file(&database).unwrap();
    for version in [0, 8, 16] {
        let connection = Connection::open(&database).unwrap();
        connection
            .pragma_update(None, "user_version", version)
            .unwrap();
        drop(connection);
        assert!(matches!(
            IntegrationService::upgrade_existing(&state),
            Err(IntegrationError::Invalid(_))
        ));
        let connection = Connection::open(&database).unwrap();
        assert_eq!(
            connection
                .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            version
        );
        assert_eq!(
            connection
                .query_row::<u32, _, _>(
                    "SELECT count(*) FROM sqlite_master WHERE type='table'",
                    [],
                    |row| row.get(0)
                )
                .unwrap(),
            0
        );
        drop(connection);
        fs::remove_file(&database).unwrap();
    }
}

#[test]
fn explicit_upgrade_preserves_owned_config_receipts_and_is_idempotent() {
    let f = Fixture::new();
    let plan = f.install();
    let installation = f.service.installed(&plan.installation_id).unwrap();
    let before = fs::read(&f.config).unwrap();
    f.service.connection().unwrap().execute_batch("DROP TABLE message_handoffs; DROP TABLE message_observations; DROP TABLE outgoing_messages; PRAGMA user_version=9").unwrap();
    let report = IntegrationService::upgrade_existing(&f.service.state).unwrap();
    assert_eq!(
        report,
        StateUpgradeReport {
            previous_schema: 9,
            current_schema: 15,
            changed: true
        }
    );
    let upgraded = IntegrationService::open_existing(&f.service.state).unwrap();
    assert_eq!(
        upgraded.installed(&plan.installation_id).unwrap().spec,
        installation.spec
    );
    assert_eq!(fs::read(&f.config).unwrap(), before);
    assert_eq!(
        IntegrationService::upgrade_existing(&f.service.state).unwrap(),
        StateUpgradeReport {
            previous_schema: 15,
            current_schema: 15,
            changed: false
        }
    );
    assert_eq!(fs::read(&f.config).unwrap(), before);
}

#[test]
fn explicit_upgrade_refuses_attached_bindings_and_unapplied_plans() {
    let f = Fixture::new();
    let installation = f.install();
    let config = fs::read(&f.config).unwrap();
    let connection = f.service.connection().unwrap();
    // Storage fixture only. No native session, identity or readiness is claimed.
    let reference = serde_json::to_string(&super::super::SessionRef {
        installation_id: installation.installation_id,
        installation_revision: "fixture".into(),
        native_session_id: "fixture".into(),
        invocation_id: None,
        start_cursor: 1,
    })
    .unwrap();
    connection
        .execute(
            "INSERT INTO session_bindings VALUES ('fixture',?1,0)",
            [&reference],
        )
        .unwrap();
    connection.pragma_update(None, "user_version", 9).unwrap();
    assert!(matches!(
        IntegrationService::upgrade_existing(&f.service.state),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(
        connection
            .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        9
    );
    connection
        .execute_batch("UPDATE session_bindings SET detached=1")
        .unwrap();
    for phase in ["planned", "prepared"] {
        connection
            .execute(
                "INSERT INTO install_plans VALUES ('pending','fixture',?1,'{}')",
                [phase],
            )
            .unwrap();
        assert!(
            matches!(IntegrationService::upgrade_existing(&f.service.state), Err(IntegrationError::RecoveryRequired(id)) if id == "pending")
        );
        assert_eq!(
            connection
                .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
                .unwrap(),
            9
        );
        connection
            .execute("DELETE FROM install_plans WHERE id='pending'", [])
            .unwrap();
    }
    assert!(
        IntegrationService::upgrade_existing(&f.service.state)
            .unwrap()
            .changed
    );
    assert_eq!(
        connection
            .query_row::<String, _, _>(
                "SELECT reference FROM session_bindings WHERE id='fixture'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        reference
    );
    assert_eq!(fs::read(&f.config).unwrap(), config);
}

#[test]
fn explicit_upgrade_rollback_retains_old_schema_on_late_sql_failure() {
    let f = Fixture::new();
    let connection = f.service.connection().unwrap();
    connection.execute_batch("DROP TABLE outgoing_messages; DROP TABLE message_handoffs; DROP TABLE message_observations; CREATE TABLE message_observations(broken TEXT); PRAGMA user_version=9").unwrap();
    assert!(matches!(
        IntegrationService::upgrade_existing(&f.service.state),
        Err(IntegrationError::Sql(_))
    ));
    assert_eq!(
        connection
            .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        9
    );
    assert_eq!(connection.query_row::<u32, _, _>("SELECT count(*) FROM sqlite_master WHERE name IN ('outgoing_messages','message_handoffs')", [], |row| row.get(0)).unwrap(), 0);
}

#[test]
fn explicit_upgrade_refuses_missing_core_history_and_another_writer() {
    let f = Fixture::new();
    let connection = f.service.connection().unwrap();
    connection.execute_batch("PRAGMA user_version=9").unwrap();
    let guard = f.service.state_lock().unwrap();
    assert!(matches!(
        IntegrationService::upgrade_existing(&f.service.state),
        Err(IntegrationError::Conflict(_))
    ));
    drop(guard);
    connection
        .execute_batch("DROP TABLE hook_invocations")
        .unwrap();
    assert!(matches!(
        IntegrationService::upgrade_existing(&f.service.state),
        Err(IntegrationError::Sql(_))
    ));
    assert_eq!(
        connection
            .pragma_query_value::<u32, _>(None, "user_version", |row| row.get(0))
            .unwrap(),
        9
    );
    assert_eq!(
        connection
            .query_row::<u32, _, _>(
                "SELECT count(*) FROM sqlite_master WHERE name='hook_invocations'",
                [],
                |row| row.get(0)
            )
            .unwrap(),
        0
    );
}

struct Fixture {
    _dir: tempfile::TempDir,
    service: IntegrationService,
    spec: InstallSpec,
    config: PathBuf,
}

fn tool_spec(f: &Fixture, id: &str) -> super::super::AgentToolSpec {
    super::super::AgentToolSpec {
        application_id: f.spec.application_id.clone(),
        installation_id: id.into(),
        tool_name: "review".into(),
        description: "Selected review operations".into(),
        input_schema: json!({"type":"object"}),
        instructions: "Keep exact 雪 instructions".into(),
        handler: HookCommand {
            executable: std::env::current_exe().unwrap(),
            arguments: vec!["inspect".into()],
        },
        timeout_ms: 5000,
    }
}

#[test]
fn owned_tool_preview_update_legacy_hook_update_and_removal_preserve_unrelated_files() {
    let f = Fixture::new();
    let original = json!({"permissions":{"deny":["Bash(rm *)"]},"env":{"OTHER":"preserve"}});
    f.write(&original);
    let hooks = f.install();
    let spec = tool_spec(&f, &hooks.installation_id);
    let plugin = super::super::render_agent_tool_plugin("claude", &spec).unwrap();
    let root = f.spec.workspace.join(".claude/skills").join(plugin.name());
    let tool = f
        .service
        .plan_agent_tool(&hooks.installation_id, spec.clone())
        .unwrap();
    assert_eq!(tool.files.len(), 3);
    assert!(tool
        .files
        .iter()
        .all(|file| file.before_fingerprint == "absent"));
    assert!(!root.exists());
    f.service.apply_install(&tool.id).unwrap();
    assert!(root.join(".claude-plugin/plugin.json").is_file());
    fs::write(root.join("user-file.txt"), "preserve exact").unwrap();
    let mut updated = spec;
    updated.instructions.push_str(" Updated instructions.");
    let update = f
        .service
        .plan_agent_tool(&hooks.installation_id, updated)
        .unwrap();
    assert_eq!(update.files.len(), 2);
    f.service.apply_install(&update.id).unwrap();
    let module = fs::read(root.join("hooks/register.js")).unwrap();
    let mut incompatible = f.spec.clone();
    incompatible
        .handler
        .arguments
        .push("changed-handler".into());
    assert!(matches!(
        f.service.plan_install(incompatible),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(fs::read(root.join("hooks/register.js")).unwrap(), module);
    let hook_update = f.service.plan_install(f.spec.clone()).unwrap();
    assert!(hook_update.files.is_empty());
    f.service.apply_install(&hook_update.id).unwrap();
    assert_eq!(fs::read(root.join("hooks/register.js")).unwrap(), module);
    let remove = f.service.plan_remove(&hooks.installation_id).unwrap();
    assert_eq!(remove.files.len(), 3);
    f.service.apply_install(&remove.id).unwrap();
    assert_eq!(f.read(), original);
    assert_eq!(
        fs::read_to_string(root.join("user-file.txt")).unwrap(),
        "preserve exact"
    );
    assert!(!root.join(".claude-plugin/plugin.json").exists());
    assert!(!root.join("hooks/register.js").exists());
    let (_, body): (String, String) = f
        .service
        .connection()
        .unwrap()
        .query_row(
            "SELECT phase,body FROM install_plans WHERE id=?1",
            [&tool.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert!(!body.contains("Keep exact"));
    assert!(serde_json::from_str::<Value>(&body).unwrap()["tool_files"]
        .as_array()
        .unwrap()
        .is_empty());
}

#[test]
fn every_tool_file_recovery_boundary_resumes_same_plan_and_retires_the_manifest_first() {
    for boundary in 1..=3 {
        let f = Fixture::new();
        let hooks = f.install();
        let spec = tool_spec(&f, &hooks.installation_id);
        let tool = f
            .service
            .plan_agent_tool(&hooks.installation_id, spec)
            .unwrap();
        let manifest = tool.files.last().unwrap().path.clone();
        assert!(manifest.ends_with(".claude-plugin/plugin.json"));
        assert!(matches!(
            f.service
                .apply_with_tool_fault(&tool.id, false, Some(boundary)),
            Err(IntegrationError::RecoveryRequired(_))
        ));
        assert_eq!(manifest.exists(), boundary == 3);
        assert!(matches!(
            f.service
                .installation_status(&hooks.installation_id)
                .unwrap(),
            InstallationStatus::RecoveryRequired { .. }
        ));
        assert!(matches!(
            f.service.plan_remove(&hooks.installation_id),
            Err(IntegrationError::RecoveryRequired(_))
        ));
        let reopened = IntegrationService::open(f._dir.path().join("private-state")).unwrap();
        assert!(matches!(
            reopened.apply_install(&tool.id).unwrap(),
            InstallationStatus::Configured { .. }
        ));
        let remove = reopened.plan_remove(&hooks.installation_id).unwrap();
        assert_eq!(remove.files.first().unwrap().path, manifest);
        assert!(matches!(
            reopened.apply_with_tool_fault(&remove.id, false, Some(boundary)),
            Err(IntegrationError::RecoveryRequired(_))
        ));
        assert!(!manifest.exists());
        assert!(matches!(
            reopened.apply_install(&remove.id).unwrap(),
            InstallationStatus::Absent
        ));
        let root = manifest.parent().unwrap().parent().unwrap();
        assert!(
            !root.exists(),
            "empty owned directories must not prevent reinstall"
        );
        let hooks = reopened.plan_install(f.spec.clone()).unwrap();
        reopened.apply_install(&hooks.id).unwrap();
        let tool = reopened
            .plan_agent_tool(
                &hooks.installation_id,
                tool_spec(&f, &hooks.installation_id),
            )
            .unwrap();
        reopened.apply_install(&tool.id).unwrap();
    }
}

#[test]
fn tool_ownership_scope_drift_and_stale_plans_refuse_without_overwriting() {
    let f = Fixture::new();
    let hooks = f.install();
    let mut spec = tool_spec(&f, &hooks.installation_id);
    spec.installation_id = "another-installation".into();
    assert!(f
        .service
        .plan_agent_tool(&hooks.installation_id, spec)
        .is_err());
    let spec = tool_spec(&f, &hooks.installation_id);
    let plugin = super::super::render_agent_tool_plugin("claude", &spec).unwrap();
    let root = f.spec.workspace.join(".claude/skills").join(plugin.name());
    fs::create_dir_all(&root).unwrap();
    assert!(matches!(
        f.service
            .plan_agent_tool(&hooks.installation_id, spec.clone()),
        Err(IntegrationError::Conflict(_))
    ));
    fs::remove_dir(&root).unwrap();
    let tool = f
        .service
        .plan_agent_tool(&hooks.installation_id, spec.clone())
        .unwrap();
    fs::create_dir_all(&root).unwrap();
    assert!(matches!(
        f.service.apply_install(&tool.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(fs::read_dir(&root).unwrap().next().is_none());
    fs::remove_dir(&root).unwrap();
    let mut config = f.read();
    config["unrelated"] = json!("edited after plan");
    f.write(&config);
    assert!(matches!(
        f.service.apply_install(&tool.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(!root.exists());
    let tool = f
        .service
        .plan_agent_tool(&hooks.installation_id, spec.clone())
        .unwrap();
    f.service.apply_install(&tool.id).unwrap();
    let module = root.join("hooks/register.js");
    let mut changed = spec.clone();
    changed.instructions.push_str(" updated");
    let update = f
        .service
        .plan_agent_tool(&hooks.installation_id, changed)
        .unwrap();
    let hook_file = root.join("hooks/hooks.json");
    let saved_hook_file = fs::read(&hook_file).unwrap();
    let saved_module = fs::read(&module).unwrap();
    fs::write(&hook_file, "changed unchanged file").unwrap();
    assert!(matches!(
        f.service.apply_install(&update.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(fs::read(&module).unwrap(), saved_module);
    fs::write(&hook_file, saved_hook_file).unwrap();
    fs::write(&module, "user edit").unwrap();
    assert!(matches!(
        f.service
            .installation_status(&hooks.installation_id)
            .unwrap(),
        InstallationStatus::Drifted { .. }
    ));
    assert!(f.service.plan_remove(&hooks.installation_id).is_err());
    assert!(f.service.plan_install(f.spec.clone()).is_err());
    assert!(f
        .service
        .plan_agent_tool(&hooks.installation_id, spec)
        .is_err());
    assert_eq!(fs::read_to_string(module).unwrap(), "user edit");
}

#[test]
fn external_edit_during_partial_tool_recovery_does_not_become_owned_after_bytes() {
    let f = Fixture::new();
    let hooks = f.install();
    let tool = f
        .service
        .plan_agent_tool(
            &hooks.installation_id,
            tool_spec(&f, &hooks.installation_id),
        )
        .unwrap();
    assert!(f
        .service
        .apply_with_tool_fault(&tool.id, false, Some(1))
        .is_err());
    fs::write(&tool.files[0].path, "external edit").unwrap();
    assert!(matches!(
        f.service.apply_install(&tool.id),
        Err(IntegrationError::Conflict(_))
    ));
    assert!(!tool.files.last().unwrap().path.exists());
    assert_eq!(
        fs::read_to_string(&tool.files[0].path).unwrap(),
        "external edit"
    );
}

#[test]
fn completion_policy_preview_update_remove_restore_prior_value_and_preserve_other_settings() {
    let f = Fixture::new();
    let original = json!({"env":{"CLAUDE_CODE_STOP_HOOK_BLOCK_CAP":"12","OTHER":"retain"},"permissions":{"deny":["Bash(rm *)"]},"hooks":{"Stop":[{"hooks":[{"type":"command","command":"user-hook"}]}]}});
    f.write(&original);
    let policy = CompletionBlockingPolicy::RequireUnbounded;
    let plan = f
        .service
        .plan_install_with_completion_policy(f.spec.clone(), policy)
        .unwrap();
    assert_eq!(f.read(), original);
    assert_eq!(plan.completion_policy, policy);
    assert_eq!(
        plan.settings,
        vec![SettingChange {
            pointer: "/env/CLAUDE_CODE_STOP_HOOK_BLOCK_CAP".into(),
            before: Some(json!("12")),
            after: Some(json!("0"))
        }]
    );
    let status = f.service.apply_install(&plan.id).unwrap();
    let InstallationStatus::Configured { installation } = status else {
        panic!("expected configured")
    };
    assert_eq!(installation.completion_policy, policy);
    assert_eq!(f.read()["env"]["CLAUDE_CODE_STOP_HOOK_BLOCK_CAP"], "0");
    let mut changed = f.spec.clone();
    changed.handler.arguments.push("updated".into());
    let update = f.service.plan_install(changed).unwrap();
    assert_eq!(update.completion_policy, policy);
    assert!(update.settings.is_empty());
    f.service.apply_install(&update.id).unwrap();
    let remove = f.service.plan_remove(&plan.installation_id).unwrap();
    assert_eq!(remove.settings[0].after, Some(json!("12")));
    f.service.apply_install(&remove.id).unwrap();
    assert_eq!(f.read(), original);
}

#[test]
fn completion_policy_preserves_absence_empty_environment_and_existing_zero() {
    for original in [
        json!({}),
        json!({"env":{}}),
        json!({"env":{"CLAUDE_CODE_STOP_HOOK_BLOCK_CAP":"0","OTHER":"retain"}}),
    ] {
        let f = Fixture::new();
        f.write(&original);
        let plan = f
            .service
            .plan_install_with_completion_policy(
                f.spec.clone(),
                CompletionBlockingPolicy::RequireUnbounded,
            )
            .unwrap();
        f.service.apply_install(&plan.id).unwrap();
        let remove = f.service.plan_remove(&plan.installation_id).unwrap();
        f.service.apply_install(&remove.id).unwrap();
        assert_eq!(f.read(), original);
    }
}

#[test]
fn completion_policy_drift_and_stale_plan_never_overwrite_external_changes() {
    let f = Fixture::new();
    f.write(&json!({"env":{"OTHER":"retain"}}));
    let plan = f
        .service
        .plan_install_with_completion_policy(
            f.spec.clone(),
            CompletionBlockingPolicy::RequireUnbounded,
        )
        .unwrap();
    f.write(&json!({"env":{"OTHER":"changed"}}));
    assert!(matches!(
        f.service.apply_install(&plan.id),
        Err(IntegrationError::Conflict(_))
    ));
    let plan = f
        .service
        .plan_install_with_completion_policy(
            f.spec.clone(),
            CompletionBlockingPolicy::RequireUnbounded,
        )
        .unwrap();
    f.service.apply_install(&plan.id).unwrap();
    let mut value = f.read();
    value["env"]["CLAUDE_CODE_STOP_HOOK_BLOCK_CAP"] = json!("1");
    f.write(&value);
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
    assert_eq!(f.read(), value);
}

#[test]
fn completion_policy_shared_zero_cannot_be_silently_restored_to_a_limit_under_another_hook() {
    let f = Fixture::new();
    f.write(&json!({"env":{"CLAUDE_CODE_STOP_HOOK_BLOCK_CAP":"8"}}));
    let policy = CompletionBlockingPolicy::RequireUnbounded;
    let first = f
        .service
        .plan_install_with_completion_policy(f.spec.clone(), policy)
        .unwrap();
    f.service.apply_install(&first.id).unwrap();
    let other = IntegrationService::open(f._dir.path().join("other-state")).unwrap();
    let mut spec = f.spec.clone();
    spec.application_id = "other-app".into();
    spec.handler.arguments.push("other-app".into());
    let second = other
        .plan_install_with_completion_policy(spec, policy)
        .unwrap();
    other.apply_install(&second.id).unwrap();
    assert!(matches!(
        f.service
            .installation_status(&first.installation_id)
            .unwrap(),
        InstallationStatus::Configured { .. }
    ));
    let before = f.read();
    assert!(matches!(
        f.service.plan_remove(&first.installation_id),
        Err(IntegrationError::Conflict(_))
    ));
    assert_eq!(f.read(), before);
    let mut update = f.spec.clone();
    update.handler.arguments.push("updated-first-app".into());
    let update = f
        .service
        .plan_install_with_completion_policy(update, policy)
        .unwrap();
    f.service.apply_install(&update.id).unwrap();
    let remove_second = other.plan_remove(&second.installation_id).unwrap();
    other.apply_install(&remove_second.id).unwrap();
    assert_eq!(f.read()["env"]["CLAUDE_CODE_STOP_HOOK_BLOCK_CAP"], "0");
    let remove_first = f.service.plan_remove(&first.installation_id).unwrap();
    f.service.apply_install(&remove_first.id).unwrap();
    assert_eq!(f.read()["env"]["CLAUDE_CODE_STOP_HOOK_BLOCK_CAP"], "8");
    assert!(f.read().pointer("/hooks/Stop").is_none());
}

#[test]
fn completion_policy_recovers_prepared_write_and_legacy_records_default_without_new_policy() {
    let f = Fixture::new();
    let original = json!({"env":{"OTHER":"retain"}});
    f.write(&original);
    let plan = f
        .service
        .plan_install_with_completion_policy(
            f.spec.clone(),
            CompletionBlockingPolicy::RequireUnbounded,
        )
        .unwrap();
    assert!(matches!(
        f.service.apply_inner(&plan.id, true),
        Err(IntegrationError::RecoveryRequired(_))
    ));
    assert_eq!(f.read()["env"]["CLAUDE_CODE_STOP_HOOK_BLOCK_CAP"], "0");
    let reopened = IntegrationService::open(f._dir.path().join("private-state")).unwrap();
    reopened.apply_install(&plan.id).unwrap();
    let remove = reopened.plan_remove(&plan.installation_id).unwrap();
    reopened.apply_install(&remove.id).unwrap();
    assert_eq!(f.read(), original);
    let legacy = reopened.plan_install(f.spec.clone()).unwrap();
    reopened.connection().unwrap().execute_batch("UPDATE install_plans SET body=json_remove(body,'$.preview.settings','$.preview.completion_policy','$.next_receipt.completion','$.next_receipt.installation.completion_policy') WHERE phase='planned';").unwrap();
    let InstallationStatus::Configured { installation } =
        reopened.apply_install(&legacy.id).unwrap()
    else {
        panic!("configured")
    };
    assert_eq!(
        installation.completion_policy,
        CompletionBlockingPolicy::NativeDefault
    );
    assert_eq!(f.read()["env"], original["env"]);
}

#[test]
fn completion_policy_requires_claude_stop_and_refuses_malformed_environment() {
    let f = Fixture::new();
    let policy = CompletionBlockingPolicy::RequireUnbounded;
    let mut wrong = f.spec.clone();
    wrong.agent_key = "cursor".into();
    assert!(matches!(
        f.service.plan_install_with_completion_policy(wrong, policy),
        Err(IntegrationError::Unsupported(_))
    ));
    let mut wrong = f.spec.clone();
    wrong.events = vec![HookEvent::BeforeTool];
    assert!(matches!(
        f.service.plan_install_with_completion_policy(wrong, policy),
        Err(IntegrationError::Invalid(_))
    ));
    f.write(&json!({"env":"invalid"}));
    assert!(matches!(
        f.service
            .plan_install_with_completion_policy(f.spec.clone(), policy),
        Err(IntegrationError::Invalid(_))
    ));
    assert_eq!(f.read(), json!({"env":"invalid"}));
    for invalid in [Value::Null, json!(0), json!(false), json!({})] {
        let original = json!({"env":{"CLAUDE_CODE_STOP_HOOK_BLOCK_CAP":invalid,"OTHER":"retain"}});
        f.write(&original);
        assert!(matches!(
            f.service
                .plan_install_with_completion_policy(f.spec.clone(), policy),
            Err(IntegrationError::Invalid(_))
        ));
        assert_eq!(f.read(), original);
    }
}

#[test]
fn completion_policy_explicit_native_default_releases_control_but_legacy_updates_preserve_it() {
    let f = Fixture::new();
    f.write(&json!({"env":{"CLAUDE_CODE_STOP_HOOK_BLOCK_CAP":"9"}}));
    let plan = f
        .service
        .plan_install_with_completion_policy(
            f.spec.clone(),
            CompletionBlockingPolicy::RequireUnbounded,
        )
        .unwrap();
    f.service.apply_install(&plan.id).unwrap();
    let unchanged = f.service.plan_install(f.spec.clone()).unwrap();
    assert_eq!(
        unchanged.completion_policy,
        CompletionBlockingPolicy::RequireUnbounded
    );
    f.service.apply_install(&unchanged.id).unwrap();
    let release = f
        .service
        .plan_install_with_completion_policy(
            f.spec.clone(),
            CompletionBlockingPolicy::NativeDefault,
        )
        .unwrap();
    assert_eq!(release.settings[0].after, Some(json!("9")));
    f.service.apply_install(&release.id).unwrap();
    assert_eq!(f.read()["env"]["CLAUDE_CODE_STOP_HOOK_BLOCK_CAP"], "9");
    let remove = f.service.plan_remove(&plan.installation_id).unwrap();
    assert!(remove.settings.is_empty());
    f.service.apply_install(&remove.id).unwrap();
    assert_eq!(f.read()["env"]["CLAUDE_CODE_STOP_HOOK_BLOCK_CAP"], "9");
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
    assert_eq!(version, 15);
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
        .execute_batch("PRAGMA user_version=16;")
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
