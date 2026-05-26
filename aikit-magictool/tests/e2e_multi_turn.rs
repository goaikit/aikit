mod common;

/// AC22 — Multi-turn E2E test, gated on OPENAI_API_KEY.
///
/// Runs the full start → message → finalize flow for `test/form_fill` and
/// asserts that `finalize` returns 200 with a `draft` that passes the tool's
/// outputSchema validation.
///
/// Skipped automatically when OPENAI_API_KEY is absent.
#[cfg(feature = "agent")]
mod e2e_multi_turn {
    use aikit_magictool::{router, validate_value};
    use axum::{body::Body, http::Request};
    use serde_json::json;
    use tower::ServiceExt;

    fn skip_without_key() -> bool {
        std::env::var("OPENAI_API_KEY").is_err()
    }

    #[tokio::test]
    #[ignore = "requires OPENAI_API_KEY and a reachable LLM; run with --include-ignored"]
    async fn multi_turn_start_message_finalize_returns_valid_draft() {
        if skip_without_key() {
            eprintln!("SKIP: OPENAI_API_KEY not set");
            return;
        }

        let state = super::common::fixture_state();
        let output_schema = state
            .registry
            .get("test", "form_fill")
            .expect("test/form_fill must be registered")
            .output_schema
            .clone();
        let output_validator =
            jsonschema::validator_for(&output_schema).expect("output schema must compile");

        // ── Step 1: start a session ────────────────────────────────────────────

        let start_payload = json!({
            "raw_text": "Low priority closed task about updating documentation. \
                         Needs tags: docs. Mark as inactive."
        });

        let app1 = router(state.clone());
        let req1 = Request::builder()
            .method("POST")
            .uri("/aitools/test/form_fill/sessions")
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(Body::from(start_payload.to_string()))
            .unwrap();

        let resp1 = app1.oneshot(req1).await.unwrap();
        let status1 = resp1.status();
        let bytes1 = axum::body::to_bytes(resp1.into_body(), usize::MAX)
            .await
            .unwrap();
        let body1: serde_json::Value =
            serde_json::from_slice(&bytes1).expect("start response must be valid JSON");

        assert_eq!(
            status1,
            axum::http::StatusCode::OK,
            "expected 200 on start, got {status1}: {body1}"
        );

        let session_id = body1["session_id"]
            .as_str()
            .expect("start response must include session_id")
            .to_owned();

        assert!(!session_id.is_empty(), "session_id must not be empty");

        // ── Step 2: send a follow-up message ───────────────────────────────────

        let app2 = router(state.clone());
        let req2 = Request::builder()
            .method("POST")
            .uri(format!(
                "/aitools/test/form_fill/sessions/{session_id}/messages"
            ))
            .header("content-type", "application/json")
            .header("accept", "application/json")
            .body(Body::from(
                json!({"content": "Please also set priority to 2 and add the tag 'review'."})
                    .to_string(),
            ))
            .unwrap();

        let resp2 = app2.oneshot(req2).await.unwrap();
        let status2 = resp2.status();
        let bytes2 = axum::body::to_bytes(resp2.into_body(), usize::MAX)
            .await
            .unwrap();
        let body2: serde_json::Value =
            serde_json::from_slice(&bytes2).expect("message response must be valid JSON");

        assert_eq!(
            status2,
            axum::http::StatusCode::OK,
            "expected 200 on message, got {status2}: {body2}"
        );

        assert!(
            body2["reply"].is_string(),
            "message response must include reply: {body2}"
        );

        // ── Step 3: finalize → extract validated Draft ─────────────────────────

        let app3 = router(state.clone());
        let req3 = Request::builder()
            .method("POST")
            .uri(format!(
                "/aitools/test/form_fill/sessions/{session_id}/finalize"
            ))
            .body(Body::empty())
            .unwrap();

        let resp3 = app3.oneshot(req3).await.unwrap();
        let status3 = resp3.status();
        let bytes3 = axum::body::to_bytes(resp3.into_body(), usize::MAX)
            .await
            .unwrap();
        let body3: serde_json::Value =
            serde_json::from_slice(&bytes3).expect("finalize response must be valid JSON");

        assert_eq!(
            status3,
            axum::http::StatusCode::OK,
            "expected 200 on finalize, got {status3}: {body3}"
        );

        let draft = &body3["draft"];
        assert!(
            draft.is_object(),
            "finalize draft must be an object: {body3}"
        );

        validate_value(&output_validator, draft)
            .unwrap_or_else(|errs| panic!("draft failed outputSchema: {}", errs.join("; ")));
    }
}
