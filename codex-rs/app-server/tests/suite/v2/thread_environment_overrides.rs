// codeks 0010: verify explicit environment selection before resume and fork initialization.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_fake_rollout_with_source;
use app_test_support::rollout_path;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadEnvironment;
use codex_app_server_protocol::ThreadForkParams;
use codex_app_server_protocol::ThreadForkResponse;
use codex_app_server_protocol::ThreadHistoryMode;
use codex_app_server_protocol::ThreadLoadedListParams;
use codex_app_server_protocol::ThreadLoadedListResponse;
use codex_app_server_protocol::ThreadReadParams;
use codex_app_server_protocol::ThreadReadResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use codex_app_server_protocol::TurnCompletedNotification;
use codex_app_server_protocol::TurnEnvironmentParams;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::UserInput;
use codex_features::Feature;
use codex_protocol::ThreadId;
use codex_protocol::protocol::MultiAgentVersion;
use codex_protocol::protocol::SessionSource;
use codex_protocol::protocol::SubAgentSource;
use codex_rollout::RolloutItem;
use codex_rollout::append_rollout_item_to_path;
use codex_rollout::read_session_meta_line;
use codex_state::StateRuntime;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_path_uri::PathUri;
use core_test_support::responses;
use pretty_assertions::assert_eq;
use serde_json::json;
use tempfile::TempDir;
use test_case::test_case;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::timeout;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

const RPC_TIMEOUT: Duration = Duration::from_secs(20);
const REMOTE_ID: &str = "remote-test";
const LOCAL_INSTRUCTIONS: &str = "LOCAL_WORKSPACE_INSTRUCTIONS";
const REMOTE_INSTRUCTIONS: &str = "REMOTE_WORKSPACE_INSTRUCTIONS";
const REMOTE_SKILL: &str = "remote-fixture-skill";

struct MixedEnvironments {
    _root: TempDir,
    _executor: Child,
    _mcp: MockServer,
    codex_home: PathBuf,
    local: TurnEnvironmentParams,
    remote: TurnEnvironmentParams,
}

impl MixedEnvironments {
    async fn new(model_url: &str) -> Result<Self> {
        let root = TempDir::new()?;
        let codex_home = root.path().join("host-home");
        let executor_home = root.path().join("executor-home");
        let local_cwd = root.path().join("local-workspace");
        let remote_cwd = root.path().join("remote-workspace");
        for directory in [&codex_home, &executor_home, &local_cwd, &remote_cwd] {
            std::fs::create_dir_all(directory)?;
        }
        std::fs::write(local_cwd.join("AGENTS.md"), LOCAL_INSTRUCTIONS)?;
        std::fs::write(remote_cwd.join("AGENTS.md"), REMOTE_INSTRUCTIONS)?;
        let skill_dir = remote_cwd.join(".agents/skills").join(REMOTE_SKILL);
        std::fs::create_dir_all(&skill_dir)?;
        std::fs::write(
            skill_dir.join("SKILL.md"),
            format!(
                "---\nname: {REMOTE_SKILL}\ndescription: Inspect the selected remote workspace.\n---\nUse the selected executor.\n"
            ),
        )?;

        let mcp = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/mcp"))
            .respond_with(|request: &wiremock::Request| {
                let request: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
                let result = match request["method"].as_str() {
                    Some("initialize") => json!({
                        "protocolVersion": "2025-11-25",
                        "capabilities": { "tools": {} },
                        "serverInfo": { "name": "remote_fixture", "version": "1.0.0" }
                    }),
                    Some("tools/list") => json!({ "tools": [{
                        "name": "inspect", "description": "Inspect this executor.",
                        "inputSchema": { "type": "object" }
                    }] }),
                    Some("resources/list") => json!({ "resources": [] }),
                    Some("resources/templates/list") => json!({ "resourceTemplates": [] }),
                    _ => return ResponseTemplate::new(202),
                };
                ResponseTemplate::new(200).set_body_json(json!({
                    "jsonrpc": "2.0", "id": request["id"], "result": result
                }))
            })
            .mount(&mcp)
            .await;
        std::fs::write(
            executor_home.join("config.toml"),
            format!(
                "[mcp_servers.remote_fixture]\nurl = \"{}/mcp\"\nrequired = true\n",
                mcp.uri()
            ),
        )?;
        let mut executor = Command::new(codex_utils_cargo_bin::cargo_bin("codex")?)
            .args(["exec-server", "--listen", "ws://127.0.0.1:0"])
            .env("CODEX_HOME", &executor_home)
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdout = executor.stdout.take().context("executor stdout")?;
        let mut lines = BufReader::new(stdout).lines();
        let url = timeout(RPC_TIMEOUT, lines.next_line())
            .await??
            .context("executor WebSocket URL")?;
        std::fs::write(
            codex_home.join("environments.toml"),
            format!(
                "default = \"local\"\ninclude_local = true\n[[environments]]\nid = \"{REMOTE_ID}\"\nurl = {}\n",
                serde_json::to_string(&url)?
            ),
        )?;
        MockResponsesConfig::new(model_url)
            .enable_feature(Feature::Goals)
            .with_sandbox_mode("danger-full-access")
            .write(&codex_home)?;
        Ok(Self {
            _root: root,
            _executor: executor,
            _mcp: mcp,
            codex_home,
            local: TurnEnvironmentParams {
                environment_id: "local".to_string(),
                cwd: AbsolutePathBuf::from_absolute_path(local_cwd.canonicalize()?)?.into(),
                runtime_workspace_roots: None,
            },
            remote: TurnEnvironmentParams {
                environment_id: REMOTE_ID.to_string(),
                cwd: AbsolutePathBuf::from_absolute_path(remote_cwd.canonicalize()?)?.into(),
                runtime_workspace_roots: None,
            },
        })
    }

    async fn app(&self) -> Result<TestAppServer> {
        TestAppServer::builder()
            .with_codex_home(&self.codex_home)
            .without_auto_env()
            .without_managed_config()
            .build_initialized_with_timeout(RPC_TIMEOUT)
            .await
    }

    async fn seed(&self) -> Result<(TestAppServer, String, String)> {
        let mut app = self.app().await?;
        let started: ThreadStartResponse = app
            .request(|request_id| ClientRequest::ThreadStart {
                request_id,
                params: ThreadStartParams {
                    cwd: Some(self.local.cwd.to_string()),
                    history_mode: Some(ThreadHistoryMode::Paginated),
                    environments: Some(vec![]),
                    ..Default::default()
                },
            })
            .await?;
        let completed = timeout(
            RPC_TIMEOUT,
            app.start_turn_and_wait_for_completion(text_turn(&started.thread.id)),
        )
        .await??;
        Ok((app, started.thread.id, completed.turn.id))
    }

    fn selection(&self, id: &str) -> TurnEnvironmentParams {
        if id == "local" {
            self.local.clone()
        } else {
            self.remote.clone()
        }
    }

    async fn activate_goal(&self, thread_id: &str) -> Result<()> {
        let state = StateRuntime::init(
            codex_state::SqliteConfig::new_for_testing(AbsolutePathBuf::from_absolute_path(
                &self.codex_home,
            )?),
            "mock_provider".to_string(),
        )
        .await?;
        state
            .thread_goals()
            .replace_thread_goal(
                ThreadId::from_string(thread_id)?,
                "Inspect the selected workspace.",
                codex_state::ThreadGoalStatus::Active,
                Some(1),
            )
            .await?;
        Ok(())
    }
}

fn text_turn(thread_id: &str) -> TurnStartParams {
    TurnStartParams {
        thread_id: thread_id.to_string(),
        input: vec![UserInput::Text {
            text: "Inspect the workspace.".to_string(),
            text_elements: vec![],
        }],
        ..Default::default()
    }
}

fn reported_environment(selection: &TurnEnvironmentParams) -> ThreadEnvironment {
    ThreadEnvironment {
        environment_id: selection.environment_id.clone(),
        cwd: selection.cwd.clone(),
        runtime_workspace_roots: vec![selection.cwd.clone()],
    }
}

async fn model_responses(turns: usize) -> (MockServer, responses::ResponseMock) {
    let server = responses::start_mock_server().await;
    let capture = responses::mount_sse_sequence(
        &server,
        ["seed", "selected"]
            .into_iter()
            .take(turns)
            .map(|id| {
                responses::sse(vec![
                    responses::ev_response_created(id),
                    responses::ev_assistant_message(&format!("message-{id}"), "Done"),
                    responses::ev_completed_with_tokens(id, 10),
                ])
            })
            .collect(),
    )
    .await;
    (server, capture)
}

fn assert_selected_context(capture: &responses::ResponseMock, remote: bool, local: bool) {
    let requests = capture.requests();
    assert_eq!(requests.len(), 2);
    let request = &requests[1];
    let context = request.message_input_texts("user").join("\n");
    assert_eq!(context.contains(REMOTE_INSTRUCTIONS), remote);
    assert_eq!(context.contains(LOCAL_INSTRUCTIONS), local);
    assert_eq!(request.body_contains_text(REMOTE_SKILL), remote);
    assert_eq!(
        request
            .tool_by_name("mcp__remote_fixture", "inspect")
            .is_some(),
        remote
    );
}

#[test_case(Some("remote-test"); "remote_only")]
#[test_case(Some("local"); "local_only")]
#[test_case(Some(""); "no_environments")]
#[test_case(None; "omitted_keeps_defaults")]
#[tokio::test]
async fn cold_resume_selects_environments_before_loading_context(
    selected: Option<&str>,
) -> Result<()> {
    let (server, capture) = model_responses(2).await;
    let fixture = MixedEnvironments::new(&server.uri()).await?;
    let (mut app, thread_id, _) = fixture.seed().await?;
    timeout(RPC_TIMEOUT, app.shutdown_gracefully()).await??;
    drop(app);
    let mut app = fixture.app().await?;
    let environments = selected.map(|id| {
        if id.is_empty() {
            vec![]
        } else {
            vec![fixture.selection(id)]
        }
    });
    let response: ThreadResumeResponse = app
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread_id.clone(),
                environments: environments.clone(),
                exclude_turns: true,
                ..Default::default()
            },
        })
        .await?;
    let expected = environments
        .map(|selections| selections.iter().map(reported_environment).collect())
        .unwrap_or_else(|| {
            vec![
                reported_environment(&fixture.local),
                ThreadEnvironment {
                    environment_id: REMOTE_ID.to_string(),
                    ..reported_environment(&fixture.local)
                },
            ]
        });
    assert_eq!(response.thread.environments, Some(expected));
    if let Some(id) = selected.filter(|id| !id.is_empty()) {
        assert!(
            response.instruction_sources.contains(
                &PathUri::try_from(fixture.selection(id).cwd)?
                    .join("AGENTS.md")?
                    .into()
            )
        );
    }
    timeout(
        RPC_TIMEOUT,
        app.start_turn_and_wait_for_completion(text_turn(&thread_id)),
    )
    .await??;
    if selected.is_some() {
        assert_selected_context(
            &capture,
            selected == Some(REMOTE_ID),
            selected == Some("local"),
        );
    } else {
        assert_eq!(capture.requests().len(), 2);
    }
    Ok(())
}

#[test_case(false; "ordinary_fork")]
#[test_case(true; "historical_fork")]
#[tokio::test]
async fn fork_selects_environments_before_loading_context(historical: bool) -> Result<()> {
    let (server, capture) = model_responses(2).await;
    let fixture = MixedEnvironments::new(&server.uri()).await?;
    let (mut app, thread_id, turn_id) = fixture.seed().await?;
    let response: ThreadForkResponse = app
        .request(|request_id| ClientRequest::ThreadFork {
            request_id,
            params: ThreadForkParams {
                thread_id,
                before_turn_id: historical.then_some(turn_id),
                environments: Some(vec![fixture.remote.clone()]),
                exclude_turns: true,
                ..Default::default()
            },
        })
        .await?;
    assert_eq!(
        response.thread.environments,
        Some(vec![reported_environment(&fixture.remote)])
    );
    assert!(
        response.instruction_sources.contains(
            &PathUri::try_from(fixture.remote.cwd.clone())?
                .join("AGENTS.md")?
                .into()
        )
    );
    timeout(
        RPC_TIMEOUT,
        app.start_turn_and_wait_for_completion(text_turn(&response.thread.id)),
    )
    .await??;
    assert_selected_context(&capture, true, false);
    Ok(())
}

#[test_case("remote-test"; "remote_goal")]
#[test_case("local"; "local_goal")]
#[tokio::test]
async fn resumed_goal_first_turn_uses_only_selected_environment(selected: &str) -> Result<()> {
    let (server, capture) = model_responses(2).await;
    let fixture = MixedEnvironments::new(&server.uri()).await?;
    let (mut app, thread_id, _) = fixture.seed().await?;
    timeout(RPC_TIMEOUT, app.shutdown_gracefully()).await??;
    drop(app);
    fixture.activate_goal(&thread_id).await?;
    let mut app = fixture.app().await?;
    let response: ThreadResumeResponse = app
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread_id.clone(),
                environments: Some(vec![fixture.selection(selected)]),
                exclude_turns: true,
                ..Default::default()
            },
        })
        .await?;
    assert_eq!(
        response.thread.environments,
        Some(vec![reported_environment(&fixture.selection(selected))])
    );
    let completed: TurnCompletedNotification =
        timeout(RPC_TIMEOUT, app.read_notification("turn/completed")).await??;
    assert_eq!(completed.thread_id, thread_id);
    assert_selected_context(&capture, selected == REMOTE_ID, selected == "local");
    Ok(())
}

#[tokio::test]
async fn loaded_resume_rejoins_same_selection_and_rejects_changes() -> Result<()> {
    let (server, capture) = model_responses(2).await;
    let fixture = MixedEnvironments::new(&server.uri()).await?;
    let (mut app, thread_id, _) = fixture.seed().await?;
    timeout(RPC_TIMEOUT, app.shutdown_gracefully()).await??;
    drop(app);
    let mut app = fixture.app().await?;
    for environments in [
        Some(vec![fixture.remote.clone()]),
        Some(vec![fixture.remote.clone()]),
        None,
    ] {
        let response: ThreadResumeResponse = app
            .request(|request_id| ClientRequest::ThreadResume {
                request_id,
                params: ThreadResumeParams {
                    thread_id: thread_id.clone(),
                    environments,
                    exclude_turns: true,
                    ..Default::default()
                },
            })
            .await?;
        assert_eq!(
            response.thread.environments,
            Some(vec![reported_environment(&fixture.remote)])
        );
    }
    for selection in [
        fixture.local.clone(),
        TurnEnvironmentParams {
            cwd: fixture.local.cwd.clone(),
            ..fixture.remote.clone()
        },
        TurnEnvironmentParams {
            runtime_workspace_roots: Some(vec![fixture.local.cwd.clone()]),
            ..fixture.remote.clone()
        },
    ] {
        let request = app
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: thread_id.clone(),
                environments: Some(vec![selection]),
                exclude_turns: true,
                ..Default::default()
            })
            .await?;
        let error: JSONRPCError = timeout(
            RPC_TIMEOUT,
            app.read_stream_until_error_message(RequestId::Integer(request)),
        )
        .await??;
        assert_eq!(
            error.error.message,
            "cannot resume loaded thread with different environments"
        );
    }
    let read: ThreadReadResponse = app
        .request(|request_id| ClientRequest::ThreadRead {
            request_id,
            params: ThreadReadParams {
                thread_id: thread_id.clone(),
                include_turns: false,
            },
        })
        .await?;
    assert_eq!(
        read.thread.environments,
        Some(vec![reported_environment(&fixture.remote)])
    );
    timeout(
        RPC_TIMEOUT,
        app.start_turn_and_wait_for_completion(text_turn(&thread_id)),
    )
    .await??;
    assert_selected_context(&capture, true, false);
    Ok(())
}

#[tokio::test]
async fn unknown_environment_rejects_resume_and_fork_without_starting_goal() -> Result<()> {
    let (server, capture) = model_responses(1).await;
    let fixture = MixedEnvironments::new(&server.uri()).await?;
    let (mut app, thread_id, _) = fixture.seed().await?;
    timeout(RPC_TIMEOUT, app.shutdown_gracefully()).await??;
    drop(app);
    fixture.activate_goal(&thread_id).await?;
    let mut app = fixture.app().await?;
    for method in ["thread/resume", "thread/fork"] {
        let request = app
            .send_request(
                method,
                Some(json!({
                    "threadId": thread_id,
                    "environments": [{
                        "environmentId": "missing-executor",
                        "cwd": fixture.remote.cwd,
                    }],
                    "excludeTurns": true,
                })),
            )
            .await?;
        let error: JSONRPCError = timeout(
            RPC_TIMEOUT,
            app.read_stream_until_error_message(RequestId::Integer(request)),
        )
        .await??;
        assert!(
            error
                .error
                .message
                .contains("unknown turn environment id `missing-executor`")
        );
        let loaded: ThreadLoadedListResponse = app
            .request(|request_id| ClientRequest::ThreadLoadedList {
                request_id,
                params: ThreadLoadedListParams::default(),
            })
            .await?;
        assert!(loaded.data.is_empty());
        assert_eq!(capture.requests().len(), 1);
        assert!(
            fixture
                ._mcp
                .received_requests()
                .await
                .context("MCP request recording")?
                .is_empty()
        );
    }
    Ok(())
}

#[tokio::test]
async fn cold_child_resume_preserves_parent_control() -> Result<()> {
    let server = responses::start_mock_server().await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&server.uri()).write(home.path())?;
    let timestamp = "2026-08-20T12-00-00";
    let thread_id = create_fake_rollout_with_source(
        home.path(),
        timestamp,
        "2026-08-20T12:00:00Z",
        "Saved child task",
        Some("mock_provider"),
        None,
        SessionSource::SubAgent(SubAgentSource::ThreadSpawn {
            parent_thread_id: ThreadId::new(),
            depth: 1,
            agent_path: None,
            agent_nickname: None,
            agent_role: None,
        }),
    )?;
    let path = rollout_path(home.path(), timestamp, &thread_id);
    let mut meta = read_session_meta_line(&path).await?;
    meta.meta.multi_agent_version = Some(MultiAgentVersion::V2);
    append_rollout_item_to_path(&path, &RolloutItem::SessionMeta(Box::new(meta))).await?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized_with_timeout(RPC_TIMEOUT)
        .await?;
    for (environments, expected) in [
        (
            Some(vec![]),
            "cannot override environments when resuming a parent-controlled sub-agent; omit environments and resume through its parent",
        ),
        (
            None,
            "cannot resume an unloaded multi-agent v2 sub-agent through its parent; resume the parent first, or use thread/read to inspect it",
        ),
    ] {
        let request = app
            .send_thread_resume_request(ThreadResumeParams {
                thread_id: thread_id.clone(),
                environments,
                exclude_turns: true,
                ..Default::default()
            })
            .await?;
        let error = timeout(
            RPC_TIMEOUT,
            app.read_stream_until_error_message(RequestId::Integer(request)),
        )
        .await??;
        assert_eq!(error.error.message, expected);
    }
    let loaded: ThreadLoadedListResponse = app
        .request(|request_id| ClientRequest::ThreadLoadedList {
            request_id,
            params: ThreadLoadedListParams::default(),
        })
        .await?;
    assert!(loaded.data.is_empty());
    assert!(
        server
            .received_requests()
            .await
            .context("model request recording")?
            .iter()
            .all(|request| !request.url.path().ends_with("/responses"))
    );
    Ok(())
}
