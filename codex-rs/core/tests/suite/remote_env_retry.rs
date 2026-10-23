// codeks 0011: retry a failed selected executor on the next task without changing its workspace.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use codex_features::Feature;
use core_test_support::exec_server::ExecServerProcess;
use core_test_support::responses;
use core_test_support::test_codex::test_codex;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::time::timeout;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::connect_async;
use tokio_util::task::AbortOnDropHandle;

const TURN_TIMEOUT: Duration = Duration::from_secs(30);
const EXECUTOR_MARKER: &str = "CODEKS_RETRY_EXECUTOR_MARKER";
const EXECUTOR_VALUE: &str = "recovered-executor";

async fn relay_executor(
    listener: TcpListener,
    target: String,
    online: Arc<AtomicBool>,
    startup_calls: Arc<Mutex<Vec<String>>>,
) -> Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        if !online.load(Ordering::Acquire) {
            // Fail the handshake while keeping the same address available for recovery.
            drop(stream);
            continue;
        }
        let mut client = accept_async(stream).await?;
        let (mut executor, _) = connect_async(&target).await?;
        loop {
            tokio::select! {
                message = client.next() => {
                    let Some(message) = message else { break };
                    let message = message?;
                    if (message.is_text() || message.is_binary())
                        && let Ok(request) = serde_json::from_slice::<Value>(&message.clone().into_data())
                        && let Some(method @ ("initialize" | "environment/info")) = request["method"].as_str()
                    {
                        startup_calls.lock().unwrap().push(method.to_string());
                    }
                    executor.send(message).await?;
                }
                message = executor.next() => {
                    let Some(message) = message else { break };
                    client.send(message?).await?;
                }
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn next_task_recovers_the_same_executor_after_initial_connection_failure() -> Result<()> {
    let executor = ExecServerProcess::start_with_env(&[(EXECUTOR_MARKER, EXECUTOR_VALUE)]).await?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("ws://{}", listener.local_addr()?);
    let online = Arc::new(AtomicBool::new(false));
    let startup_calls = Arc::new(Mutex::new(Vec::new()));
    let _relay = AbortOnDropHandle::new(tokio::spawn(relay_executor(
        listener,
        executor.websocket_url.clone(),
        Arc::clone(&online),
        Arc::clone(&startup_calls),
    )));
    let server = responses::start_mock_server().await;
    let capture = responses::mount_sse_sequence(
        &server,
        ["offline", "recovered", "healthy"]
            .into_iter()
            .flat_map(|phase| {
                vec![
                    responses::sse(vec![
                        responses::ev_response_created(phase),
                        responses::ev_function_call(
                            phase,
                            "exec_command",
                            &json!({
                                "cmd": format!("printf '%s' \"${EXECUTOR_MARKER}\""),
                                "environment_id": "remote",
                                "login": false,
                                "yield_time_ms": 1000,
                            })
                            .to_string(),
                        ),
                        responses::ev_completed(phase),
                    ]),
                    responses::sse(vec![
                        responses::ev_assistant_message(&format!("{phase}-message"), "Done."),
                        responses::ev_completed(&format!("{phase}-done")),
                    ]),
                ]
            })
            .collect(),
    )
    .await;
    let test = timeout(
        TURN_TIMEOUT,
        test_codex()
            .with_exec_server_url(url)
            .with_config(|config| {
                config.features.enable(Feature::UnifiedExec).unwrap();
                config.features.disable(Feature::DeferredExecutor).unwrap();
            })
            .build(&server),
    )
    .await??;
    let selected = test.codex.environment_selections().await;
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].environment_id, "remote");

    timeout(
        TURN_TIMEOUT,
        test.submit_turn("Try the executor while it is offline."),
    )
    .await??;
    let offline = capture
        .function_call_output_text("offline")
        .context("offline command result")?;
    assert!(
        offline.contains("No usable execution environment is available"),
        "{offline}"
    );
    assert!(startup_calls.lock().unwrap().is_empty());

    online.store(true, Ordering::Release);
    timeout(
        TURN_TIMEOUT,
        test.submit_turn("Try the same executor again."),
    )
    .await??;
    let recovered = capture
        .function_call_output_text("recovered")
        .context("recovered command result")?;
    assert!(
        recovered.contains("Process exited with code 0"),
        "{recovered}"
    );
    assert!(recovered.ends_with(EXECUTOR_VALUE), "{recovered}");
    assert_eq!(test.codex.environment_selections().await, selected);
    let successful_startup = startup_calls.lock().unwrap().clone();
    assert_eq!(
        successful_startup
            .iter()
            .filter(|method| *method == "initialize")
            .count(),
        1
    );

    timeout(
        TURN_TIMEOUT,
        test.submit_turn("Use the healthy executor once more."),
    )
    .await??;
    let healthy = capture
        .function_call_output_text("healthy")
        .context("healthy command result")?;
    assert!(healthy.contains("Process exited with code 0"), "{healthy}");
    assert!(healthy.ends_with(EXECUTOR_VALUE), "{healthy}");
    assert_eq!(test.codex.environment_selections().await, selected);
    assert_eq!(*startup_calls.lock().unwrap(), successful_startup);
    Ok(())
}
