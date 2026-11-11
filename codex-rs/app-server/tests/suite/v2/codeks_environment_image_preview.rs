// codeks 0021: native reads use the selected executor without host-path fallback.
use super::exec_server_test_support::accept_initialized_exec_server;
use super::exec_server_test_support::read_exec_server_json;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_app_server_protocol::FsReadFileResponse;
use codex_app_server_protocol::RequestId;
use futures::SinkExt;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::time::Duration;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;

const RPC_TIMEOUT: Duration = Duration::from_secs(20);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_file_routes_foreign_paths_and_preserves_executor_errors() -> Result<()> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let executor_url = format!("ws://{}", listener.local_addr()?);
    let codex_home = TempDir::new()?;
    let host_path = codex_home.path().join("host.txt");
    std::fs::write(&host_path, "host contents")?;
    let mut app = TestAppServer::builder()
        .with_codex_home(codex_home.path())
        .without_auto_env()
        .build_initialized()
        .await?;
    let executor = tokio::spawn(async move {
        let mut websocket = accept_initialized_exec_server(listener).await?;
        let mut reads = 0;
        loop {
            let request = read_exec_server_json(&mut websocket).await?;
            let response = match request["method"].as_str() {
                Some("environment/info") => json!({
                    "id": request["id"], "result": {
                        "shell": {"name": "powershell", "path": "C:\\Windows\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"}
                    }
                }),
                Some("fs/readFile") => {
                    let expected = [
                        "file:///C:/screenshots/result.png",
                        "file://files/share/result.png",
                        "file:///C:/screenshots/missing.png",
                    ][reads];
                    assert_eq!(request["params"]["path"], expected);
                    assert_eq!(request["params"]["sandbox"], serde_json::Value::Null);
                    reads += 1;
                    if reads == 3 {
                        json!({"id": request["id"], "error": {"code": -32000, "message": "image missing on executor"}})
                    } else {
                        json!({"id": request["id"], "result": {"dataBase64": STANDARD.encode("executor image")}})
                    }
                }
                other => anyhow::bail!("unexpected executor method: {other:?}"),
            };
            websocket
                .send(Message::Text(response.to_string().into()))
                .await?;
            if reads == 3 {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    });
    let add_id = app
        .send_raw_request(
            "environment/add",
            Some(json!({
                "environmentId": "remote-a", "execServerUrl": executor_url
            })),
        )
        .await?;
    timeout(
        RPC_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(add_id)),
    )
    .await??;

    for path in [r"C:\screenshots\result.png", r"\\files\share\result.png"] {
        let id = app
            .send_raw_request(
                "fs/readFile",
                Some(json!({
                    "path": path, "environmentId": "remote-a"
                })),
            )
            .await?;
        let result: FsReadFileResponse = to_response(
            timeout(
                RPC_TIMEOUT,
                app.read_stream_until_response_message(RequestId::Integer(id)),
            )
            .await??,
        )?;
        assert_eq!(STANDARD.decode(result.data_base64)?, b"executor image");
    }

    for environment_id in [None, Some("local")] {
        let mut params = json!({"path": host_path});
        if let Some(environment_id) = environment_id {
            params["environmentId"] = json!(environment_id);
        }
        let id = app.send_raw_request("fs/readFile", Some(params)).await?;
        let result: FsReadFileResponse = to_response(
            timeout(
                RPC_TIMEOUT,
                app.read_stream_until_response_message(RequestId::Integer(id)),
            )
            .await??,
        )?;
        assert_eq!(STANDARD.decode(result.data_base64)?, b"host contents");
    }

    let unknown_id = app
        .send_raw_request(
            "fs/readFile",
            Some(json!({
                "path": host_path, "environmentId": "unknown"
            })),
        )
        .await?;
    let unknown = timeout(
        RPC_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(unknown_id)),
    )
    .await??;
    assert_eq!(unknown.error.message, "unknown environment id `unknown`");

    let missing_id = app
        .send_raw_request(
            "fs/readFile",
            Some(json!({
                "path": "C:\\screenshots\\missing.png", "environmentId": "remote-a"
            })),
        )
        .await?;
    let missing = timeout(
        RPC_TIMEOUT,
        app.read_stream_until_error_message(RequestId::Integer(missing_id)),
    )
    .await??;
    assert!(missing.error.message.contains("image missing on executor"));
    timeout(RPC_TIMEOUT, executor).await???;
    Ok(())
}

#[tokio::test]
async fn read_file_rejects_relative_paths_without_using_host_cwd() -> Result<()> {
    let mut app = TestAppServer::builder()
        .without_auto_env()
        .build_initialized()
        .await?;
    for environment_id in [None, Some("local")] {
        let id = app
            .send_raw_request(
                "fs/readFile",
                Some(json!({
                    "path": "relative.png", "environmentId": environment_id
                })),
            )
            .await?;
        let error = timeout(
            RPC_TIMEOUT,
            app.read_stream_until_error_message(RequestId::Integer(id)),
        )
        .await??;
        assert_eq!(error.error.code, -32600);
    }
    Ok(())
}
