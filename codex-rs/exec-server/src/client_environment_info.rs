// codeks 0026: acquire a fresh session for a new read-only metadata query.
use super::ExecServerError;
use super::LazyRemoteExecServerClient;
use super::RecoveryPolicy;
use crate::protocol::EnvironmentInfo;

impl LazyRemoteExecServerClient {
    pub(crate) async fn force_environment_info(&self) -> Result<EnvironmentInfo, ExecServerError> {
        let client = self.get().await?;
        match client.force_environment_info().await {
            Err(ExecServerError::Disconnected(_))
                if matches!(self.recovery_policy, RecoveryPolicy::Wait) && self.can_reconnect() =>
            {
                match client.inner.rpc_client().await {
                    Ok(_) | Err(ExecServerError::Disconnected(_)) => {
                        self.get().await?.force_environment_info().await
                    }
                    Err(error) => Err(error),
                }
            }
            result => result,
        }
    }
}
