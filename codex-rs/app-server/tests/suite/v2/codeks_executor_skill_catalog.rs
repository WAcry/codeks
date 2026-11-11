// codeks 0019: catalog queries are passive and preserve executor identity and policy.
use super::exec_server_test_support::accept_initialized_exec_server;
use super::exec_server_test_support::read_exec_server_json;
use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::SkillsCatalogResponse;
use codex_app_server_protocol::ThreadListResponse;
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
async fn catalog_keeps_native_policy_and_paths_without_creating_threads() -> Result<()> {
    let home = TempDir::new()?;
    let host_skill = home.path().join("skills/review");
    std::fs::create_dir_all(host_skill.join("agents"))?;
    std::fs::write(
        host_skill.join("SKILL.md"),
        "---\nname: review\ndescription: Host review.\n---\nHost instructions.\n",
    )?;
    std::fs::write(
        host_skill.join("agents/openai.yaml"),
        "policy:\n  allow_implicit_invocation: false\n",
    )?;
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("ws://{}", listener.local_addr()?);
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .without_auto_env()
        .build_initialized()
        .await?;
    let executor = tokio::spawn(async move {
        let mut websocket = accept_initialized_exec_server(listener).await?;
        let mut discoveries = 0;
        loop {
            let request = read_exec_server_json(&mut websocket).await?;
            let result = match request["method"].as_str() {
                Some("environment/info") => json!({
                    "shell": {"name": "powershell", "path": "powershell.exe"},
                    "executorVersion": "0.167.1",
                    "capabilities": {"capabilityDiscoveryV2": true}
                }),
                Some("capabilities/discoverV2") => {
                    assert_eq!(request["params"]["cwd"], "file:///C:/Users/test");
                    discoveries += 1;
                    let mut result = json!({"plugins": [], "skills": [{
                        "name": "review", "description": "Executor review.",
                        "path": "file:///C:/Users/test/.agents/skills/review/SKILL.md",
                        "discoveryRoot": {"id": "user-agent-skills", "path": "file:///C:/Users/test/.agents/skills"},
                        "dependencies": null,
                        "policy": {"allowImplicitInvocation": false, "products": []}
                    }], "warnings": []});
                    if discoveries == 1 {
                        result["disabledSkillPaths"] =
                            json!(["file:///C:/Users/test/.agents/skills/review/SKILL.md"]);
                    }
                    result
                }
                other => anyhow::bail!("unexpected executor method: {other:?}"),
            };
            websocket
                .send(Message::Text(
                    json!({"id": request["id"], "result": result})
                        .to_string()
                        .into(),
                ))
                .await?;
            if discoveries == 2 {
                break;
            }
        }
        Ok::<_, anyhow::Error>(())
    });
    let id = app
        .send_raw_request(
            "environment/add",
            Some(json!({
                "environmentId": "computer", "execServerUrl": url
            })),
        )
        .await?;
    timeout(
        RPC_TIMEOUT,
        app.read_stream_until_response_message(RequestId::Integer(id)),
    )
    .await??;

    for supported in [true, false] {
        let id = app
            .send_raw_request(
                "skills/catalog",
                Some(json!({
                    "environments": [
                        {"environmentId": "local", "cwd": home.path()},
                        {"environmentId": "computer", "cwd": "C:\\Users\\test"},
                        {"environmentId": "missing", "cwd": "C:\\Users\\test"}
                    ], "forceReload": true
                })),
            )
            .await?;
        let response: SkillsCatalogResponse = to_response(
            timeout(
                RPC_TIMEOUT,
                app.read_stream_until_response_message(RequestId::Integer(id)),
            )
            .await??,
        )?;
        assert_eq!(response.data.len(), 3);
        let host = response.data[0]
            .skills
            .iter()
            .find(|skill| skill.name == "review")
            .unwrap();
        assert_eq!(host.description, "Host review.");
        assert!(host.enabled);
        assert!(!host.allow_implicit_invocation);
        assert!(!host.selector.starts_with("skill://"));
        let remote = &response.data[1];
        assert_eq!(remote.environment_id, "computer");
        assert_eq!(remote.supports_skill_config, supported);
        assert!(remote.errors.is_empty());
        assert_eq!(remote.skills.len(), 1);
        let skill = &remote.skills[0];
        assert_eq!(skill.description, "Executor review.");
        assert_eq!(
            skill.path.as_str(),
            r"C:\Users\test\.agents\skills\review\SKILL.md"
        );
        assert_eq!(
            skill.selector,
            "skill://user-agent-skills/C:/Users/test/.agents/skills/review/SKILL.md"
        );
        assert_eq!(skill.enabled, !supported);
        assert!(!skill.allow_implicit_invocation);
        assert_eq!(
            response.data[2].errors,
            vec!["unknown environment id `missing`"]
        );
    }
    let id = app.send_raw_request("thread/list", Some(json!({}))).await?;
    let threads: ThreadListResponse = to_response(
        timeout(
            RPC_TIMEOUT,
            app.read_stream_until_response_message(RequestId::Integer(id)),
        )
        .await??,
    )?;
    assert!(
        threads.data.is_empty(),
        "catalog inspection must not create a conversation"
    );
    timeout(RPC_TIMEOUT, executor).await???;
    Ok(())
}
