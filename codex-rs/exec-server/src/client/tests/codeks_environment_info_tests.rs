// codeks 0026: retry only read-only metadata after native session recovery settles.

use super::*;
use crate::protocol::ENVIRONMENT_INFO_METHOD;
use codex_exec_server_protocol::JSONRPCError;
use codex_exec_server_protocol::JSONRPCErrorError;
use codex_exec_server_protocol::RequestId;
use pretty_assertions::assert_eq;

fn lazy_client(listener: &TcpListener) -> LazyRemoteExecServerClient {
    LazyRemoteExecServerClient::new(
        ExecServerTransportParams::WebSocketUrl {
            websocket_url: format!(
                "ws://{}",
                listener
                    .local_addr()
                    .expect("listener should have an address")
            ),
            connect_timeout: Duration::from_secs(1),
            initialize_timeout: Duration::from_secs(1),
            http_headers: HeaderMap::new(),
        },
        HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
    )
}

async fn info_request(socket: &mut WebSocketStream<TcpStream>) -> RequestId {
    let JSONRPCMessage::Request(request) = read_jsonrpc_websocket(socket).await else {
        panic!("expected an environment info request");
    };
    assert_eq!(request.method, ENVIRONMENT_INFO_METHOD);
    request.id
}

async fn reply_error(socket: &mut WebSocketStream<TcpStream>, id: RequestId, message: &str) {
    write_jsonrpc_websocket(
        socket,
        JSONRPCMessage::Error(JSONRPCError {
            id,
            error: JSONRPCErrorError {
                code: -32600,
                message: message.to_string(),
                data: None,
            },
        }),
    )
    .await;
}

async fn reject_resume(socket: &mut WebSocketStream<TcpStream>) {
    let JSONRPCMessage::Request(request) = read_jsonrpc_websocket(socket).await else {
        panic!("expected a session resume request");
    };
    assert_eq!(request.method, INITIALIZE_METHOD);
    let params: crate::protocol::InitializeParams =
        serde_json::from_value(request.params.expect("initialize params should exist"))
            .expect("initialize params should deserialize");
    assert_eq!(
        params.resume_session_id.as_deref(),
        Some("original-session")
    );
    reply_error(socket, request.id, "unknown session id original-session").await;
}

async fn no_more_work(
    listener: &TcpListener,
    socket: &mut WebSocketStream<TcpStream>,
    finished: oneshot::Receiver<()>,
) {
    tokio::select! {
        result = finished => result.expect("test should finish"),
        message = socket.next() => panic!("unexpected request or disconnect: {message:?}"),
        connection = listener.accept() => panic!("unexpected replacement connection: {connection:?}"),
    }
}

#[test_case::test_case(false; "resumed_session")]
#[test_case::test_case(true; "expired_session")]
#[tokio::test]
async fn info_read_recovers_without_replaying_old_processes(expired: bool) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let client = lazy_client(&listener);
    let expected = EnvironmentInfo::local();
    let response = expected.clone();
    let (recovering_tx, recovering_rx) = oneshot::channel();
    let (allow_recovery_tx, allow_recovery_rx) = oneshot::channel();
    let (finished_tx, finished_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut original = accept_websocket(&listener).await;
        complete_websocket_initialize(&mut original, "original-session", None).await;
        if expired {
            let JSONRPCMessage::Request(start) = read_jsonrpc_websocket(&mut original).await else {
                panic!("expected the original process start");
            };
            assert_eq!(start.method, EXEC_METHOD);
            let params: ExecParams =
                serde_json::from_value(start.params.expect("process params should exist"))
                    .expect("process params should deserialize");
            write_jsonrpc_websocket(
                &mut original,
                JSONRPCMessage::Response(JSONRPCResponse {
                    id: start.id,
                    result: serde_json::to_value(ExecResponse {
                        process_id: params.process_id,
                        sandbox_type: None,
                    })
                    .expect("process response should serialize"),
                }),
            )
            .await;
        }
        info_request(&mut original).await;
        drop(original);

        let mut resumed = accept_websocket(&listener).await;
        recovering_tx.send(()).expect("recovery should be observed");
        allow_recovery_rx
            .await
            .expect("recovery should be released");
        let mut current = if expired {
            reject_resume(&mut resumed).await;
            drop(resumed);
            let mut replacement = accept_websocket(&listener).await;
            complete_websocket_initialize(&mut replacement, "replacement-session", None).await;
            replacement
        } else {
            complete_websocket_initialize(
                &mut resumed,
                "original-session",
                Some("original-session"),
            )
            .await;
            resumed
        };
        // A replacement must receive only metadata, never the original process/start.
        let id = info_request(&mut current).await;
        write_jsonrpc_websocket(
            &mut current,
            JSONRPCMessage::Response(JSONRPCResponse {
                id,
                result: serde_json::to_value(response).expect("info should serialize"),
            }),
        )
        .await;
        no_more_work(&listener, &mut current, finished_rx).await;
    });

    let original = client
        .get()
        .await
        .expect("initial connection should succeed");
    let old_process = if expired {
        Some(
            original
                .start_process(
                    ExecParams {
                        process_id: ProcessId::from("original-process"),
                        metadata: None,
                        argv: vec!["never-replay".to_string()],
                        cwd: PathUri::from_host_native_path(
                            std::env::current_dir().expect("current directory"),
                        )
                        .expect("current directory URI"),
                        env_policy: None,
                        shell_snapshot: None,
                        env: HashMap::new(),
                        tty: false,
                        pipe_stdin: true,
                        arg0: None,
                        sandbox: None,
                        enforce_managed_network: false,
                        managed_network: None,
                        network_proxy: None,
                    },
                    None,
                )
                .await
                .expect("original process should start"),
        )
    } else {
        None
    };
    let read = tokio::spawn({
        let client = client.clone();
        async move { client.force_environment_info().await }
    });
    timeout(Duration::from_secs(2), recovering_rx)
        .await
        .expect("recovery should start")
        .expect("recovery should be observed");
    assert!(!original.is_disconnected());
    assert!(
        !read.is_finished(),
        "metadata should wait for existing recovery"
    );
    allow_recovery_tx.send(()).expect("recovery should proceed");
    let actual = timeout(Duration::from_secs(2), read)
        .await
        .expect("metadata read should finish")
        .expect("metadata task should finish")
        .expect("one metadata call should recover");
    assert_eq!(actual, expected);
    let current = client.get().await.expect("current client should be usable");
    assert_eq!(
        current.session_id().as_deref(),
        Some(if expired {
            "replacement-session"
        } else {
            "original-session"
        })
    );
    if let Some(process) = old_process {
        let failure = process
            .read(None, None, Some(0))
            .await
            .expect("old process read");
        assert!(failure.closed);
        assert!(failure.failure.is_some());
        assert!(matches!(
            process.write(b"never replay".to_vec()).await,
            Err(ExecServerError::Disconnected(_))
        ));
    }
    finished_tx.send(()).expect("server should finish");
    server.await.expect("server task should finish");
}

#[test_case::test_case(false; "application_error")]
#[test_case::test_case(true; "malformed_response")]
#[tokio::test]
async fn info_errors_do_not_retry_without_a_disconnect(malformed: bool) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let client = lazy_client(&listener);
    let (finished_tx, finished_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut socket = accept_websocket(&listener).await;
        complete_websocket_initialize(&mut socket, "original-session", None).await;
        let id = info_request(&mut socket).await;
        if malformed {
            write_jsonrpc_websocket(
                &mut socket,
                JSONRPCMessage::Response(JSONRPCResponse {
                    id,
                    result: serde_json::json!({}),
                }),
            )
            .await;
        } else {
            reply_error(&mut socket, id, "metadata request rejected").await;
        }
        no_more_work(&listener, &mut socket, finished_rx).await;
    });
    let result = timeout(Duration::from_secs(2), client.force_environment_info())
        .await
        .expect("metadata error should return");
    if malformed {
        assert!(matches!(result, Err(ExecServerError::Json(_))));
    } else {
        assert!(
            matches!(result, Err(ExecServerError::Server { code: -32600, message })
            if message == "metadata request rejected")
        );
    }
    finished_tx.send(()).expect("server should finish");
    server.await.expect("server task should finish");
}

#[tokio::test]
async fn fail_fast_info_does_not_wait_for_recovery_or_replace_the_client() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let client = lazy_client(&listener);
    let (recovering_tx, recovering_rx) = oneshot::channel();
    let (allow_recovery_tx, allow_recovery_rx) = oneshot::channel();
    let (finished_tx, finished_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut original = accept_websocket(&listener).await;
        complete_websocket_initialize(&mut original, "original-session", None).await;
        info_request(&mut original).await;
        drop(original);
        let mut resumed = accept_websocket(&listener).await;
        recovering_tx.send(()).expect("recovery should be observed");
        allow_recovery_rx.await.expect("recovery should proceed");
        reject_resume(&mut resumed).await;
        tokio::select! {
            result = finished_rx => result.expect("test should finish"),
            connection = listener.accept() => panic!("fail-fast created a new session: {connection:?}"),
        }
    });
    let original = client
        .get()
        .await
        .expect("initial connection should succeed");
    let read = tokio::spawn({
        let client = client.fail_fast();
        async move { client.force_environment_info().await }
    });
    timeout(Duration::from_secs(2), recovering_rx)
        .await
        .expect("recovery should start")
        .expect("recovery should be observed");
    let result = timeout(Duration::from_secs(2), read)
        .await
        .expect("fail-fast metadata must not wait for recovery")
        .expect("metadata task should finish");
    assert!(matches!(result, Err(ExecServerError::Disconnected(_))));
    allow_recovery_tx.send(()).expect("recovery should proceed");
    assert!(original.inner.rpc_client().await.is_err());
    finished_tx.send(()).expect("server should finish");
    server.await.expect("server task should finish");
}

#[tokio::test]
async fn replacement_info_error_is_returned_without_another_attempt() {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener should bind");
    let client = lazy_client(&listener);
    let (finished_tx, finished_rx) = oneshot::channel();
    let server = tokio::spawn(async move {
        let mut original = accept_websocket(&listener).await;
        complete_websocket_initialize(&mut original, "original-session", None).await;
        info_request(&mut original).await;
        drop(original);
        let mut resumed = accept_websocket(&listener).await;
        reject_resume(&mut resumed).await;
        drop(resumed);
        let mut replacement = accept_websocket(&listener).await;
        complete_websocket_initialize(&mut replacement, "replacement-session", None).await;
        let id = info_request(&mut replacement).await;
        reply_error(&mut replacement, id, "replacement metadata rejected").await;
        no_more_work(&listener, &mut replacement, finished_rx).await;
    });
    let result = timeout(Duration::from_secs(2), client.force_environment_info())
        .await
        .expect("replacement error should return");
    assert!(
        matches!(result, Err(ExecServerError::Server { code: -32600, message })
        if message == "replacement metadata rejected")
    );
    finished_tx.send(()).expect("server should finish");
    server.await.expect("server task should finish");
}
