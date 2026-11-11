// codeks 0021: read native paths through an existing named executor connection.
use super::map_fs_error;
use crate::error_code::invalid_request;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use codex_app_server_protocol::FsReadFileResponse;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_exec_server::EnvironmentManager;
use codex_utils_path_uri::LegacyAppPathString;
use codex_utils_path_uri::PathUri;

pub(super) async fn read_file(
    environments: &EnvironmentManager,
    environment_id: &str,
    path: LegacyAppPathString,
) -> Result<FsReadFileResponse, JSONRPCErrorError> {
    let environment = environments
        .get_environment(environment_id)
        .ok_or_else(|| invalid_request(format!("unknown environment id `{environment_id}`")))?;
    let path = PathUri::try_from(path).map_err(|error| invalid_request(error.to_string()))?;
    let bytes = environment
        .get_filesystem()
        .read_file(&path, Default::default(), /*sandbox*/ None)
        .await
        .map_err(map_fs_error)?;
    Ok(FsReadFileResponse {
        data_base64: STANDARD.encode(bytes),
    })
}
