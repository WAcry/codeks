// codeks 0021: image origins survive live projection, history, and legacy records.
use crate::FsReadFileParams;
use crate::ItemStartedNotification;
use crate::ThreadHistoryBuilder;
use crate::ThreadItemPayload;
use crate::build_turns_from_rollout_items;
use codex_protocol::ThreadId;
use codex_protocol::items::ImageViewItem;
use codex_protocol::items::TurnItem;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::ItemCompletedEvent;
use codex_protocol::protocol::TurnStartedEvent;
use codex_protocol::protocol::ViewImageToolCallEvent;
use codex_rollout::RolloutItem;
use codex_utils_path_uri::LegacyAppPathString;
use codex_utils_path_uri::PathUri;
use pretty_assertions::assert_eq;
use serde_json::json;

#[test]
fn image_environment_survives_live_and_legacy_history() {
    let item = TurnItem::ImageView(ImageViewItem {
        id: "image-1".into(),
        path: PathUri::parse("file:///C:/screenshots/result.png").unwrap(),
        environment_id: Some("remote-a".into()),
    });
    let expected = ThreadItemPayload::ImageView {
        id: "image-1".into(),
        path: LegacyAppPathString::from_string(r"C:\screenshots\result.png"),
        environment_id: Some("remote-a".into()),
    };
    assert_eq!(ThreadItemPayload::from(item.clone()), expected);
    assert_eq!(
        serde_json::to_value(&expected).unwrap()["environmentId"],
        "remote-a"
    );

    let started = ItemStartedNotification::new(
        ThreadId::default().to_string(),
        "turn-1".into(),
        ThreadItemPayload::from(item.clone()),
        1,
    );
    assert_eq!(started.item.payload, expected);
    let mut live = ThreadHistoryBuilder::new();
    live.handle_live_paginated_event(&EventMsg::TurnStarted(TurnStartedEvent {
        turn_id: "turn-1".into(),
        turn_attribution: None,
        root_turn_id: None,
        trace_id: None,
        started_at: None,
        model_context_window: None,
        collaboration_mode_kind: Default::default(),
    }));
    let completed = EventMsg::ItemCompleted(ItemCompletedEvent {
        thread_id: ThreadId::default(),
        turn_id: "turn-1".into(),
        item: item.clone(),
        started_at_ms: Some(1),
        completed_at_ms: 2,
    });
    let persisted: EventMsg =
        serde_json::from_value(serde_json::to_value(completed).unwrap()).unwrap();
    // Image history uses the compatibility event; the canonical completion carries timing.
    let legacy_events = item.as_legacy_events(false);
    for event in &legacy_events {
        live.handle_live_paginated_event(event);
    }
    live.handle_live_paginated_event(&persisted);
    let turns = live.finish();
    assert_eq!(turns[0].items.len(), 1);
    assert_eq!(turns[0].items[0].payload, expected);

    let legacy = legacy_events
        .into_iter()
        .map(RolloutItem::EventMsg)
        .collect::<Vec<_>>();
    let persisted: Vec<RolloutItem> =
        serde_json::from_value(serde_json::to_value(legacy).unwrap()).unwrap();
    assert_eq!(
        build_turns_from_rollout_items(&persisted)[0].items[0].payload,
        expected
    );
}

#[test]
fn old_image_history_does_not_invent_an_environment() {
    let event: ViewImageToolCallEvent = serde_json::from_value(json!({
        "call_id": "old-image", "path": "file:///tmp/image.png"
    }))
    .unwrap();
    assert_eq!(event.environment_id, None);
    let turns = build_turns_from_rollout_items(&[RolloutItem::EventMsg(
        EventMsg::ViewImageToolCall(event),
    )]);
    let value = serde_json::to_value(&turns[0].items[0].payload).unwrap();
    assert!(value.get("environmentId").is_none());
    let public: ThreadItemPayload = serde_json::from_value(json!({
        "type": "imageView", "id": "old-image", "path": "/tmp/image.png"
    }))
    .unwrap();
    assert!(matches!(
        public,
        ThreadItemPayload::ImageView {
            environment_id: None,
            ..
        }
    ));
    let core: ImageViewItem = serde_json::from_value(json!({
        "id": "old-image", "path": "file:///tmp/image.png"
    }))
    .unwrap();
    assert_eq!(core.environment_id, None);
}

#[test]
fn read_file_accepts_foreign_native_paths_and_keeps_host_default() {
    let remote: FsReadFileParams = serde_json::from_value(json!({
        "path": "C:\\screenshots\\result.png", "environmentId": "remote-a"
    }))
    .unwrap();
    assert_eq!(remote.environment_id.as_deref(), Some("remote-a"));
    assert_eq!(
        PathUri::try_from(remote.path).unwrap().to_string(),
        "file:///C:/screenshots/result.png"
    );
    let host: FsReadFileParams =
        serde_json::from_value(json!({"path": "/tmp/result.png"})).unwrap();
    assert_eq!(host.environment_id, None);
    assert_eq!(
        serde_json::to_value(host).unwrap(),
        json!({"path": "/tmp/result.png"})
    );
}
