mod common;

/// AC21 — One-shot E2E test, gated on OPENAI_API_KEY.
///
/// Runs `POST /aitools/test/form_fill` with a realistic example payload and
/// asserts that the response is 200 with a `draft` object that passes the
/// `test/form_fill` outputSchema (covers string, string+format:textarea,
/// boolean, integer, oneOf/enum, and array field types).
///
/// Skipped automatically when OPENAI_API_KEY is absent.
#[cfg(feature = "agent")]
mod e2e_one_shot {
    use aikit_magictool::{router, validate_value};
    use axum::{body::Body, http::Request};
    use serde_json::json;
    use tower::ServiceExt;

    fn skip_without_key() -> bool {
        std::env::var("OPENAI_API_KEY").is_err()
    }

    #[tokio::test]
    #[ignore = "requires OPENAI_API_KEY and a reachable LLM; run with --include-ignored"]
    async fn post_test_form_fill_returns_valid_draft() {
        if skip_without_key() {
            eprintln!("SKIP: OPENAI_API_KEY not set");
            return;
        }

        let state = super::common::fixture_state();

        // Grab the compiled output validator from the registered tool so we can
        // validate the draft independently of the HTTP handler.
        let output_schema = state
            .registry
            .get("test", "form_fill")
            .expect("test/form_fill must be registered")
            .output_schema
            .clone();
        let output_validator =
            jsonschema::validator_for(&output_schema).expect("output schema must compile");

        let app = router(state);

        let payload = json!({
            "raw_text": "High priority open task about migrating the database. \
                         Needs tags: migration, urgent. Mark as active."
        });

        let req = Request::builder()
            .method("POST")
            .uri("/aitools/test/form_fill")
            .header("content-type", "application/json")
            .body(Body::from(payload.to_string()))
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        let status = resp.status();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let body: serde_json::Value =
            serde_json::from_slice(&bytes).expect("response must be valid JSON");

        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "expected 200, got {status}: {body}"
        );

        let draft = &body["draft"];
        assert!(draft.is_object(), "draft must be an object: {body}");

        // Validate against the compiled outputSchema (covers all field types
        // required by AC21: string, string+format:textarea, boolean, integer,
        // oneOf, array).
        validate_value(&output_validator, draft)
            .unwrap_or_else(|errs| panic!("draft failed outputSchema: {}", errs.join("; ")));
    }
}
