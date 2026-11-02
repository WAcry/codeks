// codeks 0013: changing defaults must not reroute existing threads.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStatus;
use codex_app_server_protocol::UserInput;
use codex_protocol::openai_models::ReasoningEffort;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::time::timeout;
use wiremock::MockServer;

const RPC_TIMEOUT: Duration = Duration::from_secs(20);
const ORIGINAL_MODEL: &str = "gpt-5.6-sol";
const REPLACEMENT_MODEL: &str = "gpt-5.6-luna";

fn configure_providers(home: &Path, original: &MockServer, replacement: &MockServer) -> Result<()> {
    MockResponsesConfig::new(&original.uri())
        .with_model_provider("original")
        .with_model(ORIGINAL_MODEL)
        .with_root_config("model_reasoning_effort = \"high\"")
        .with_extra_config(&format!(
            r#"
[model_providers.replacement]
name = "Replacement provider"
base_url = "{}/v1"
wire_api = "responses"
request_max_retries = 0
stream_max_retries = 0
"#,
            replacement.uri(),
        ))
        .write(home)?;
    Ok(())
}

fn change_defaults(home: &Path) -> Result<()> {
    let path = home.join("config.toml");
    let config = std::fs::read_to_string(&path)?;
    std::fs::write(
        path,
        config
            .replace(
                "model_provider = \"original\"",
                "model_provider = \"replacement\"",
            )
            .replace(
                &format!("model = \"{ORIGINAL_MODEL}\""),
                &format!("model = \"{REPLACEMENT_MODEL}\""),
            )
            .replace(
                "model_reasoning_effort = \"high\"",
                "model_reasoning_effort = \"low\"",
            ),
    )?;
    Ok(())
}

async fn app(home: &Path) -> Result<TestAppServer> {
    TestAppServer::builder()
        .with_codex_home(home)
        .without_auto_env()
        .build_initialized()
        .await
}

async fn start_thread(app: &mut TestAppServer, cwd: &Path) -> Result<ThreadStartResponse> {
    app.request(|request_id| ClientRequest::ThreadStart {
        request_id,
        params: ThreadStartParams {
            cwd: Some(cwd.to_string_lossy().into_owned()),
            // No tools are used; keep the workspace alive independently of either server.
            environments: Some(Vec::new()),
            ..Default::default()
        },
    })
    .await
}

fn saved_settings(thread_id: &str) -> ThreadResumeParams {
    ThreadResumeParams {
        thread_id: thread_id.to_string(),
        model_provider: Some("original".to_string()),
        model: Some(ORIGINAL_MODEL.to_string()),
        config: Some(HashMap::from([(
            "model_reasoning_effort".to_string(),
            json!("high"),
        )])),
        ..Default::default()
    }
}

async fn turn(app: &mut TestAppServer, thread_id: &str, text: &str) -> Result<()> {
    let completed = timeout(
        RPC_TIMEOUT,
        app.start_turn_and_wait_for_completion(TurnStartParams {
            thread_id: thread_id.to_string(),
            input: vec![UserInput::Text {
                text: text.to_string(),
                text_elements: Vec::new(),
            }],
            ..Default::default()
        }),
    )
    .await??;
    assert_eq!(completed.turn.status, TurnStatus::Completed);
    Ok(())
}

async fn capture_responses(server: &MockServer, count: usize) -> responses::ResponseMock {
    responses::mount_sse_sequence(
        server,
        (0..count)
            .map(|index| {
                responses::sse(vec![
                    responses::ev_assistant_message(&format!("message-{index}"), "Saved answer"),
                    responses::ev_completed(&format!("response-{index}")),
                ])
            })
            .collect(),
    )
    .await
}

fn assert_settings(capture: &responses::ResponseMock, count: usize, model: &str, effort: &str) {
    let requests = capture.requests();
    assert_eq!(requests.len(), count);
    for request in requests {
        let body = request.body_json();
        assert_eq!(body["model"], json!(model));
        assert_eq!(body["reasoning"]["effort"], json!(effort));
    }
}

#[tokio::test]
async fn loaded_threads_keep_their_provider_after_defaults_change() -> Result<()> {
    let original = MockServer::start().await;
    let replacement = MockServer::start().await;
    let original_requests = capture_responses(&original, 2).await;
    let replacement_requests = capture_responses(&replacement, 1).await;
    let home = TempDir::new()?;
    configure_providers(home.path(), &original, &replacement)?;
    let mut app = app(home.path()).await?;
    let started = start_thread(&mut app, home.path()).await?;
    assert_eq!(started.model_provider, "original");
    turn(
        &mut app,
        &started.thread.id,
        "Remember the original conversation.",
    )
    .await?;

    change_defaults(home.path())?;
    let resumed: ThreadResumeResponse = app
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: saved_settings(&started.thread.id),
        })
        .await?;
    assert_eq!(resumed.thread.id, started.thread.id);
    assert_eq!(resumed.model_provider, "original");
    assert_eq!(resumed.model, ORIGINAL_MODEL);
    assert_eq!(resumed.reasoning_effort, Some(ReasoningEffort::High));
    turn(
        &mut app,
        &started.thread.id,
        "Continue the original conversation.",
    )
    .await?;

    let new_thread = start_thread(&mut app, home.path()).await?;
    assert_eq!(new_thread.model_provider, "replacement");
    assert_eq!(new_thread.model, REPLACEMENT_MODEL);
    assert_eq!(new_thread.reasoning_effort, Some(ReasoningEffort::Low));
    turn(&mut app, &new_thread.thread.id, "Use the new defaults.").await?;

    assert_settings(&original_requests, 2, ORIGINAL_MODEL, "high");
    assert_settings(&replacement_requests, 1, REPLACEMENT_MODEL, "low");
    timeout(RPC_TIMEOUT, app.shutdown_gracefully()).await??;
    Ok(())
}

#[tokio::test]
async fn cold_resume_and_explicit_fork_keep_the_saved_provider() -> Result<()> {
    let original = MockServer::start().await;
    let replacement = MockServer::builder().start().await;
    let original_requests = capture_responses(&original, 3).await;
    let home = TempDir::new()?;
    configure_providers(home.path(), &original, &replacement)?;
    let mut writer = app(home.path()).await?;
    let started = start_thread(&mut writer, home.path()).await?;
    let seed = "Keep this history on the original provider.";
    turn(&mut writer, &started.thread.id, seed).await?;
    timeout(RPC_TIMEOUT, writer.shutdown_gracefully()).await??;
    drop(writer);

    change_defaults(home.path())?;
    let mut reader = app(home.path()).await?;
    let resumed: ThreadResumeResponse = reader
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: saved_settings(&started.thread.id),
        })
        .await?;
    assert_eq!(resumed.thread.id, started.thread.id);
    assert_eq!(resumed.model_provider, "original");
    assert_eq!(resumed.thread.model_provider, "original");
    assert_eq!(resumed.model, ORIGINAL_MODEL);
    assert_eq!(resumed.reasoning_effort, Some(ReasoningEffort::High));
    turn(&mut reader, &started.thread.id, "Continue after restart.").await?;

    let fork: ThreadForkResponse = reader
        .request(|request_id| ClientRequest::ThreadFork {
            request_id,
            params: ThreadForkParams {
                thread_id: started.thread.id.clone(),
                model_provider: Some("original".to_string()),
                model: Some(ORIGINAL_MODEL.to_string()),
                config: saved_settings(&started.thread.id).config,
                environments: Some(Vec::new()),
                ..Default::default()
            },
        })
        .await?;
    assert_ne!(fork.thread.id, started.thread.id);
    assert_eq!(
        fork.thread.forked_from_id.as_deref(),
        Some(started.thread.id.as_str())
    );
    assert_eq!(fork.model_provider, "original");
    assert_eq!(fork.model, ORIGINAL_MODEL);
    assert_eq!(fork.reasoning_effort, Some(ReasoningEffort::High));
    turn(&mut reader, &fork.thread.id, "Continue the explicit fork.").await?;

    assert_settings(&original_requests, 3, ORIGINAL_MODEL, "high");
    for request in original_requests.requests().iter().skip(1) {
        assert!(
            request
                .message_input_texts("user")
                .iter()
                .any(|text| text == seed)
        );
    }
    assert!(
        replacement
            .received_requests()
            .await
            .expect("recorded replacement requests")
            .iter()
            .all(|request| !request.url.path().ends_with("/responses"))
    );
    timeout(RPC_TIMEOUT, reader.shutdown_gracefully()).await??;
    Ok(())
}
