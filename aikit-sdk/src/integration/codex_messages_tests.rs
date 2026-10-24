use super::*;
use serde_json::json;

fn history() -> Value {
    json!({"thread":{"id":"thread","turns":[{"id":"turn","itemsView":"full","items":[]}]}})
}
fn inspect(queue: &Value, history: &Value) -> Result<CodexMessageInspection, IntegrationError> {
    inspect_codex_message_snapshot(
        "thread",
        "client",
        "frozen 雪",
        &serde_json::to_vec(queue).unwrap(),
        &serde_json::to_vec(history).unwrap(),
    )
}
fn queued(id: &str, text: &str) -> Value {
    json!({"id":id,"clientUserMessageId":"client","input":[{"type":"text","text":text,"text_elements":[]}]})
}
fn retained() -> Value {
    json!({"type":"userMessage","id":"message","clientId":"client","content":[{"type":"text","text":"frozen 雪","text_elements":[]}]})
}

#[test]
fn repeated_caller_ids_preserve_all_native_entries_and_changed_payload() {
    let queue = json!({"data":[queued("first","frozen 雪"),queued("second","frozen 雪"),queued("changed","changed")],"nextCursor":null});
    let result = inspect(&queue, &history()).unwrap();
    assert_eq!(result.records.len(), 3);
    assert_eq!(
        result
            .records
            .iter()
            .map(|item| item.payload_matches)
            .collect::<Vec<_>>(),
        vec![true, true, false]
    );
    assert_eq!(
        result.records[1].location,
        CodexMessageLocation::Queued {
            queue_id: "second".into()
        }
    );
    assert_eq!(result.queue_next_cursor, None);
}

#[test]
fn retained_input_is_correlated_without_selecting_it_over_overlapping_queue() {
    let mut history = history();
    history["thread"]["turns"][0]["items"] = json!([retained()]);
    let queue = json!({"data":[queued("queue","frozen 雪")],"nextCursor":"next-page"});
    let result = inspect(&queue, &history).unwrap();
    assert_eq!(result.records.len(), 2);
    assert_eq!(
        result.records[1],
        CodexMessageMatch {
            location: CodexMessageLocation::Retained {
                turn_id: "turn".into(),
                message_id: "message".into()
            },
            payload_matches: true
        }
    );
    assert_eq!(result.queue_next_cursor.as_deref(), Some("next-page"));
}

#[test]
fn partial_or_missing_history_never_becomes_an_absence_decision() {
    let mut history = history();
    history["thread"]["turns"][0]["itemsView"] = json!("summary");
    let result = inspect(&json!({"data":[],"nextCursor":null}), &history).unwrap();
    assert!(result.records.is_empty());
    assert!(result.history_has_unloaded_items);
    history["thread"]["turns"][0]
        .as_object_mut()
        .unwrap()
        .remove("itemsView");
    assert!(
        inspect(&json!({"data":[],"nextCursor":null}), &history)
            .unwrap()
            .history_has_unloaded_items
    );
    history["thread"].as_object_mut().unwrap().remove("turns");
    assert!(inspect(&json!({"data":[],"nextCursor":null}), &history).is_err());
}

#[test]
fn wrong_target_missing_cursor_and_malformed_ids_refuse() {
    let queue = json!({"data":[],"nextCursor":null});
    let mut history = history();
    history["thread"]["id"] = json!("foreign");
    assert!(inspect(&queue, &history).is_err());
    history["thread"]["id"] = json!("thread");
    assert!(inspect(&json!({"data":[]}), &history).is_err());
    assert!(inspect(
        &json!({"data":[queued("","frozen 雪")],"nextCursor":null}),
        &history
    )
    .is_err());
    assert!(inspect(&json!({"data":[],"nextCursor":42}), &history).is_err());
}

#[test]
fn attachments_annotations_unknown_framing_and_extra_text_do_not_match_frozen_bytes() {
    for input in [
        json!([{"type":"text","text":"frozen 雪","text_elements":[{"start":0}]}]),
        json!([{"type":"text","text":"frozen 雪","extra":"unqualified"}]),
        json!([{"type":"text","text":"frozen 雪"},{"type":"image","image_url":"fixture"}]),
        json!([{"type":"input_text","text":"frozen 雪"}]),
        json!([{"type":"text","text":"frozen 雪\n"}]),
    ] {
        let mut item = queued("queue", "frozen 雪");
        item["input"] = input;
        let result = inspect(&json!({"data":[item],"nextCursor":null}), &history()).unwrap();
        assert_eq!(result.records.len(), 1);
        assert!(!result.records[0].payload_matches);
    }
}

#[test]
fn unrelated_clients_and_non_user_items_do_not_match() {
    let mut item = queued("queue", "frozen 雪");
    item["clientUserMessageId"] = json!("other");
    let mut history = history();
    let mut message = retained();
    message["type"] = json!("agentMessage");
    history["thread"]["turns"][0]["items"] = json!([message]);
    assert!(inspect(&json!({"data":[item],"nextCursor":null}), &history)
        .unwrap()
        .records
        .is_empty());
}

#[test]
fn invalid_scope_and_oversized_inputs_fail_without_truncation() {
    assert!(inspect_codex_message_snapshot("", "client", "text", b"{}", b"{}").is_err());
    assert!(inspect_codex_message_snapshot("thread", "client\0", "text", b"{}", b"{}").is_err());
    assert!(inspect_codex_message_snapshot(
        "thread",
        "client",
        &"x".repeat(MESSAGE_CONTENT_LIMIT + 1),
        b"{}",
        b"{}"
    )
    .is_err());
    assert!(inspect_codex_message_snapshot(
        "thread",
        "client",
        "text",
        &vec![b' '; 4 * MESSAGE_CONTENT_LIMIT + 1],
        b"{}"
    )
    .is_err());
}
