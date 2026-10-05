use super::tests::host;
use super::*;

#[test]
fn failed_start_settles_accepted_commands_and_releases_capacity() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = host(dir.path());
    let (release, wait) = mpsc::sync_channel(1);
    let wait = Mutex::new(wait);
    Arc::get_mut(&mut h).unwrap().factory = Arc::new(move |_, _, _| {
        wait.lock().unwrap().recv().unwrap();
        anyhow::bail!("fixture startup failure")
    });
    let id = h
        .create(create_request(dir.path(), "create"))
        .unwrap()
        .session_id;
    let receipt = h
        .command(
            &id,
            SessionCommand {
                command_id: "queued".into(),
                action: SessionAction::ContextUsage,
            },
        )
        .unwrap();
    assert_eq!(receipt.status, CommandStatus::Accepted);
    release.send(()).unwrap();
    for _ in 0..200 {
        if h.live.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(h.live.lock().unwrap().is_empty());
    let receipt = h.store.lookup(&id, "queued").unwrap();
    assert_eq!(receipt.status, CommandStatus::Failed);
    assert_eq!(
        receipt.failure.unwrap().code,
        "session_closed_before_dispatch"
    );
    assert_eq!(h.store.get(&id).unwrap().status, SessionStatus::Failed);
}

fn create_request(dir: &std::path::Path, key: &str) -> CreateSession {
    CreateSession {
        command_id: key.into(),
        backend: SessionBackend::Pi,
        cwd: dir.into(),
        prompt: "fixture".into(),
        model: None,
        resume: None,
        permission_policy: PermissionPolicy::Ask,
    }
}
fn pending(h: &Host, id: &str) -> PendingRequest {
    for _ in 0..100 {
        if let Some(request) = h.store.pending(id).unwrap().into_iter().next() {
            return request;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("request did not register")
}

#[test]
fn expired_and_duplicate_responses_cannot_grant_permission() {
    let dir = tempfile::tempdir().unwrap();
    let h = host(dir.path());
    let id = h
        .create(create_request(dir.path(), "create"))
        .unwrap()
        .session_id;
    let live = h.live.lock().unwrap()[&id].clone();
    let worker = std::thread::spawn({
        let h = h.clone();
        let id = id.clone();
        let live = live.clone();
        move || h.ask(&id, &live, "permission", json!({}))
    });
    let request = pending(&h, &id);
    live.requests
        .lock()
        .unwrap()
        .get_mut(&request.request_id)
        .unwrap()
        .expires_at_ms = 0;
    let action = SessionAction::Respond {
        request_id: request.request_id,
        response: RequestResponse::Allow { option_id: None },
    };
    let receipt = h
        .command(
            &id,
            SessionCommand {
                command_id: "late".into(),
                action: action.clone(),
            },
        )
        .unwrap();
    assert_eq!(receipt.status, CommandStatus::Failed);
    assert!(matches!(worker.join().unwrap(), RequestResponse::Deny));
    assert_eq!(
        h.command(
            &id,
            SessionCommand {
                command_id: "duplicate".into(),
                action
            }
        )
        .unwrap()
        .status,
        CommandStatus::Failed
    );
    assert!(h.store.pending(&id).unwrap().is_empty());
    h.drain();
}

#[test]
fn timeout_and_close_resolve_waiters_without_a_viewer() {
    let dir = tempfile::tempdir().unwrap();
    let h = host(dir.path());
    let id = h
        .create(create_request(dir.path(), "create"))
        .unwrap()
        .session_id;
    let live = h.live.lock().unwrap()[&id].clone();
    let start = std::time::Instant::now();
    assert!(matches!(
        h.ask(&id, &live, "permission", json!({})),
        RequestResponse::Deny
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
    let waiter = std::thread::spawn({
        let h = h.clone();
        let id = id.clone();
        let live = live.clone();
        move || h.ask(&id, &live, "question", json!({}))
    });
    pending(&h, &id);
    h.command(
        &id,
        SessionCommand {
            command_id: "close".into(),
            action: SessionAction::Close,
        },
    )
    .unwrap();
    assert!(matches!(waiter.join().unwrap(), RequestResponse::Deny));
    assert!(matches!(
        h.ask(&id, &live, "permission", json!({})),
        RequestResponse::Deny
    ));
    h.drain();
}

#[test]
fn response_and_cancel_race_has_one_winner() {
    for n in 0..10 {
        let dir = tempfile::tempdir().unwrap();
        let h = host(dir.path());
        let id = h
            .create(create_request(dir.path(), "create"))
            .unwrap()
            .session_id;
        let live = h.live.lock().unwrap()[&id].clone();
        let waiter = std::thread::spawn({
            let h = h.clone();
            let id = id.clone();
            let live = live.clone();
            move || h.ask(&id, &live, "permission", json!({}))
        });
        let request = pending(&h, &id);
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let cancel = std::thread::spawn({
            let h = h.clone();
            let id = id.clone();
            let live = live.clone();
            let b = barrier.clone();
            move || {
                b.wait();
                h.cancel_requests(&id, &live);
            }
        });
        barrier.wait();
        let action = SessionAction::Respond {
            request_id: request.request_id,
            response: RequestResponse::Allow { option_id: None },
        };
        let first = h
            .command(
                &id,
                SessionCommand {
                    command_id: format!("answer-{n}"),
                    action: action.clone(),
                },
            )
            .unwrap();
        cancel.join().unwrap();
        let answer = waiter.join().unwrap();
        assert_eq!(
            matches!(answer, RequestResponse::Allow { .. }),
            first.status == CommandStatus::Dispatched
        );
        assert_eq!(
            h.command(
                &id,
                SessionCommand {
                    command_id: "again".into(),
                    action
                }
            )
            .unwrap()
            .status,
            CommandStatus::Failed
        );
        assert!(h.store.pending(&id).unwrap().is_empty());
        h.drain();
    }
}

#[test]
fn full_command_queue_rejects_without_blocking_another_session() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = host(dir.path());
    Arc::get_mut(&mut h).unwrap().max_sessions = 2;
    let (release, wait) = mpsc::sync_channel(1);
    let wait = Arc::new(Mutex::new(wait));
    struct Noop;
    impl adapters::Driver for Noop {
        fn execute(&mut self, _: &SessionAction) -> anyhow::Result<Option<Value>> {
            Ok(None)
        }
    }
    Arc::get_mut(&mut h).unwrap().factory = Arc::new(move |req, _, _| {
        if req.command_id == "blocked" {
            wait.lock().unwrap().recv().unwrap();
        }
        Ok((Box::new(Noop), adapters::capabilities(req.backend)))
    });
    let id = h
        .create(create_request(dir.path(), "blocked"))
        .unwrap()
        .session_id;
    for n in 0..32 {
        assert_eq!(
            h.command(
                &id,
                SessionCommand {
                    command_id: format!("c{n}"),
                    action: SessionAction::ContextUsage
                }
            )
            .unwrap()
            .status,
            CommandStatus::Accepted
        );
    }
    let full = h
        .command(
            &id,
            SessionCommand {
                command_id: "full".into(),
                action: SessionAction::ContextUsage,
            },
        )
        .unwrap();
    assert_eq!(full.error.as_deref(), Some("command_queue_full"));
    assert_eq!(h.metrics.queue_rejections.load(Ordering::Relaxed), 1);
    assert!(h.create(create_request(dir.path(), "other")).is_ok());
    assert!(h
        .create(create_request(dir.path(), "over-capacity"))
        .is_err());
    release.send(()).unwrap();
    h.drain();
}

#[test]
fn drain_still_allows_explicit_close_and_settles_its_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let mut h = host(dir.path());
    let (release, wait) = mpsc::sync_channel(1);
    let wait = Mutex::new(wait);
    struct Driver;
    impl adapters::Driver for Driver {
        fn execute(&mut self, _: &SessionAction) -> anyhow::Result<Option<Value>> {
            Ok(None)
        }
    }
    Arc::get_mut(&mut h).unwrap().factory = Arc::new(move |req, _, _| {
        wait.lock().unwrap().recv().unwrap();
        Ok((Box::new(Driver), adapters::capabilities(req.backend)))
    });
    let id = h
        .create(create_request(dir.path(), "create"))
        .unwrap()
        .session_id;
    h.drain();
    assert_eq!(
        h.command(
            &id,
            SessionCommand {
                command_id: "close".into(),
                action: SessionAction::Close
            }
        )
        .unwrap()
        .status,
        CommandStatus::Accepted
    );
    release.send(()).unwrap();
    for _ in 0..200 {
        if h.live.lock().unwrap().is_empty() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(
        h.store.lookup(&id, "close").unwrap().status,
        CommandStatus::Dispatched
    );
    assert_eq!(h.store.get(&id).unwrap().status, SessionStatus::Closed);
}

#[tokio::test]
async fn stalled_subscribers_release_capacity_and_do_not_block_commands() {
    use cli_framework::tower::ServiceExt;
    let dir = tempfile::tempdir().unwrap();
    let h = host(dir.path());
    let id = h
        .create(create_request(dir.path(), "create"))
        .unwrap()
        .session_id;
    for _ in 0..10 {
        h.emit(
            &id,
            SessionEventKind::Input {
                text: "replay".into(),
            },
        );
    }
    let app = router(h.clone());
    let mut responses = vec![];
    for _ in 0..4 {
        let response = app
            .clone()
            .oneshot(
                axum::http::Request::builder()
                    .uri(format!("/gateway/sessions/{id}/events"))
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(response);
    }
    let rejected = app
        .oneshot(
            axum::http::Request::builder()
                .uri(format!("/gateway/sessions/{id}/events"))
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(rejected.status(), StatusCode::SERVICE_UNAVAILABLE);
    h.command(
        &id,
        SessionCommand {
            command_id: "close".into(),
            action: SessionAction::Close,
        },
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(h.subscriptions.available_permits(), 4);
    assert_eq!(h.metrics.slow_subscribers.load(Ordering::Relaxed), 4);
    drop(responses);
    h.drain();
}
