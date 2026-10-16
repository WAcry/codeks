//! codeks 0005: custom multi-agent v2 namespaces exchange agent messages in plaintext.

use anyhow::Result;
use codex_features::Feature;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_function_call_with_namespace;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once_match;
use core_test_support::responses::namespace_child_tool;
use core_test_support::responses::received_responses_requests;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::responses::strip_metadata_from_json;
use core_test_support::responses::strip_response_item_ids_from_json;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;

const NAMESPACE: &str = "agents";
const PARENT_PROMPT: &str = "Spawn the plaintext worker.";
const CHILD_TASK: &str = "Plaintext task for the worker.";
const SPAWN_CALL_ID: &str = "spawn-plaintext-worker";

fn body_contains(request: &wiremock::Request, text: &str) -> bool {
    String::from_utf8_lossy(&request.body).contains(text)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn custom_namespace_sends_plaintext_agent_messages() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let server = start_mock_server().await;
    let spawn_args =
        json!({"message": CHILD_TASK, "task_name": "worker", "fork_turns": "none"}).to_string();
    // Without encryption markers the server returns no encrypted_function_args.
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, PARENT_PROMPT) && !body_contains(request, SPAWN_CALL_ID)
        },
        sse(vec![
            ev_response_created("parent-spawn"),
            ev_function_call_with_namespace(SPAWN_CALL_ID, NAMESPACE, "spawn_agent", &spawn_args),
            ev_completed("parent-spawn"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| body_contains(request, SPAWN_CALL_ID),
        sse(vec![
            ev_response_created("parent-done"),
            ev_assistant_message("parent-message", "The worker has started."),
            ev_completed("parent-done"),
        ]),
    )
    .await;
    mount_sse_once_match(
        &server,
        |request: &wiremock::Request| {
            body_contains(request, CHILD_TASK) && !body_contains(request, SPAWN_CALL_ID)
        },
        sse(vec![
            ev_response_created("child-done"),
            ev_assistant_message("child-message", "Done."),
            ev_completed("child-done"),
        ]),
    )
    .await;

    let test = test_codex()
        .with_config(|config| {
            for feature in [Feature::Collab, Feature::MultiAgentV2] {
                config
                    .features
                    .enable(feature)
                    .expect("enable multi-agent v2");
            }
            config
                .features
                .disable(Feature::EnableRequestCompression)
                .expect("disable request compression for mock matching");
            config.multi_agent_v2.tool_namespace = Some(NAMESPACE.to_string());
        })
        .build_with_auto_env(&server)
        .await?;
    test.submit_turn(PARENT_PROMPT).await?;

    let requests = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let requests = received_responses_requests(&server).await;
            if requests
                .iter()
                .any(|request| !request.inputs_of_type("agent_message").is_empty())
            {
                break requests;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;

    let parent_body = requests
        .iter()
        .find(|request| {
            request.body_contains_text(PARENT_PROMPT) && !request.body_contains_text(SPAWN_CALL_ID)
        })
        .expect("first parent request")
        .body_json();
    for tool_name in ["spawn_agent", "send_message", "followup_task"] {
        let tool = namespace_child_tool(&parent_body, NAMESPACE, tool_name).expect(tool_name);
        let message = &tool["parameters"]["properties"]["message"];
        assert!(
            message.is_object() && message.get("encrypted").is_none(),
            "{tool_name} message schema: {message}"
        );
    }

    let child = requests
        .iter()
        .find(|request| !request.inputs_of_type("agent_message").is_empty())
        .expect("child request");
    assert_eq!(
        strip_response_item_ids_from_json(strip_metadata_from_json(Value::Array(
            child.inputs_of_type("agent_message"),
        ))),
        json!([{
            "type": "agent_message",
            "author": "/root",
            "recipient": "/root/worker",
            "content": [{
                "type": "input_text",
                "text": format!(
                    "Message Type: NEW_TASK\nTask name: /root/worker\nSender: /root\nPayload:\n{CHILD_TASK}"
                ),
            }],
        }])
    );

    Ok(())
}
