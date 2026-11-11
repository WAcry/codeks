// codeks 0020: ordinary executor MCP config uses native stdio with executor-owned context.

use super::*;
use codex_app_server_protocol::TurnEnvironmentParams;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ordinary_executor_stdio_preserves_cwd_credentials_and_host_precedence() -> Result<()> {
    let responses_server = responses::start_mock_server().await;
    let host_home = TempDir::new()?;
    let executor_home = TempDir::new()?;
    let workspace = executor_home.path().join("workspace");
    let server_cwd = workspace.join("server");
    std::fs::create_dir_all(&server_cwd)?;
    let command = toml::Value::String(stdio_server_bin()?).to_string();
    MockResponsesConfig::new(&responses_server.uri())
        .with_sandbox_mode("danger-full-access")
        .with_root_config(&format!(
            "[mcp_servers.shared]\ncommand = {command}\nenv = {{ MCP_EXECUTOR_MARKER = \"host-config\" }}\n"
        ))
        .write(host_home.path())?;
    std::fs::write(
        executor_home.path().join("config.toml"),
        format!(
            r#"
[mcp_servers.executor_stdio]
command = {command}
cwd = "server"
env_vars = ["MCP_EXECUTOR_MARKER"]

[mcp_servers.default_cwd]
command = {command}
env = {{ MCP_EXECUTOR_MARKER = "executor-literal" }}

[mcp_servers.shared]
command = "must-not-replace-host-server"

[mcp_servers.disabled_stdio]
command = "must-not-start-disabled-server"
enabled = false
"#,
        ),
    )?;
    let mut executor = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
        .args(["exec-server", "--listen", "ws://127.0.0.1:0"])
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .env("CODEX_HOME", executor_home.path())
        .env(EXECUTOR_ENV_NAME, EXECUTOR_ENV_VALUE)
        .spawn()?;
    let mut lines =
        BufReader::new(executor.stdout.take().expect("executor stdout is piped")).lines();
    let executor_url = timeout(DEFAULT_READ_TIMEOUT, lines.next_line())
        .await??
        .expect("executor emits its websocket URL");
    std::fs::write(
        host_home.path().join("environments.toml"),
        format!(
            "include_local = true\n[[environments]]\nid = \"{EXECUTOR_ID}\"\nurl = \"{executor_url}\"\n"
        ),
    )?;
    let mut app_server = TestAppServer::builder()
        .with_codex_home(host_home.path())
        .with_env_overrides(&[(EXECUTOR_ENV_NAME, Some("host-only-canary"))])
        .without_auto_env()
        .build_initialized_with_timeout(DEFAULT_READ_TIMEOUT)
        .await?;
    let request_id = app_server
        .send_thread_start_request(ThreadStartParams {
            model: Some("mock-model".to_string()),
            environments: Some(vec![
                TurnEnvironmentParams {
                    environment_id: "local".to_string(),
                    cwd: PathUri::from_host_native_path(host_home.path())?.into(),
                    runtime_workspace_roots: None,
                },
                TurnEnvironmentParams {
                    environment_id: EXECUTOR_ID.to_string(),
                    cwd: PathUri::from_host_native_path(&workspace)?.into(),
                    runtime_workspace_roots: None,
                },
            ]),
            ..Default::default()
        })
        .await?;
    let ThreadStartResponse { thread, .. } =
        timeout(DEFAULT_READ_TIMEOUT, app_server.read_response(request_id)).await??;

    async fn call(
        app_server: &mut TestAppServer,
        thread_id: &str,
        server: &str,
        tool: &str,
        arguments: Value,
    ) -> Result<Value> {
        let request_id = app_server
            .send_mcp_server_tool_call_request(McpServerToolCallParams {
                thread_id: thread_id.to_string(),
                server: server.to_string(),
                tool: tool.to_string(),
                arguments: Some(arguments),
                meta: None,
            })
            .await?;
        let response: McpServerToolCallResponse =
            timeout(DEFAULT_READ_TIMEOUT, app_server.read_response(request_id)).await??;
        Ok(response
            .structured_content
            .expect("fixture returns structured output"))
    }

    let echo = json!({"message": "ordinary config", "env_var": EXECUTOR_ENV_NAME});
    assert_eq!(
        call(
            &mut app_server,
            &thread.id,
            "executor_stdio",
            "echo",
            echo.clone()
        )
        .await?,
        json!({"echo": "ECHOING: ordinary config", "env": EXECUTOR_ENV_VALUE})
    );
    for (server, expected) in [("executor_stdio", &server_cwd), ("default_cwd", &workspace)] {
        let result = call(&mut app_server, &thread.id, server, "cwd", json!({})).await?;
        let actual = result["cwd"].as_str().expect("fixture returns its cwd");
        assert_eq!(
            std::fs::canonicalize(actual)?,
            std::fs::canonicalize(expected)?
        );
    }
    assert_eq!(
        call(
            &mut app_server,
            &thread.id,
            "default_cwd",
            "echo",
            echo.clone()
        )
        .await?,
        json!({"echo": "ECHOING: ordinary config", "env": "executor-literal"})
    );
    assert_eq!(
        call(&mut app_server, &thread.id, "shared", "echo", echo).await?,
        json!({"echo": "ECHOING: ordinary config", "env": "host-config"})
    );
    let servers = mcp_server_statuses(&mut app_server, thread.id).await?;
    assert!(
        servers
            .iter()
            .any(|server| server.name == "disabled_stdio" && server.tools.is_empty())
    );
    Ok(())
}
