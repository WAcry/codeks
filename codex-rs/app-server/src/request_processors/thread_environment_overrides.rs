// codeks 0010: explicit resume environments must match an already loaded thread.
use super::ThreadConfigSnapshot;
use super::ThreadManager;
use super::invalid_request;
use super::resolve_turn_environment_requests;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::ThreadResumeParams;

pub(super) fn validate_loaded_thread_environments(
    thread_manager: &ThreadManager,
    params: &ThreadResumeParams,
    config_snapshot: &ThreadConfigSnapshot,
) -> Result<(), JSONRPCErrorError> {
    let Some(requested) =
        resolve_turn_environment_requests(thread_manager, params.environments.clone())?
    else {
        return Ok(());
    };
    let active = config_snapshot.environment_selections();
    if requested.len() != active.len()
        || requested.iter().zip(active).any(|(requested, active)| {
            requested.environment_id != active.environment_id
                || requested.cwd != active.cwd
                || requested.workspace_roots != active.workspace_roots
        })
    {
        return Err(invalid_request(
            "cannot resume loaded thread with different environments",
        ));
    }
    Ok(())
}
