// codeks 0020: validate executor-only credentials after catalog materialization.

use super::McpCredentialPolicy;
use super::StartupOutcomeError;
use anyhow::anyhow;
use codex_config::McpServerConfig;
use codex_config::McpServerTransportConfig;
use codex_config::types::McpServerEnvVar;
use std::collections::HashMap;

pub(super) fn validate(
    server_name: &str,
    config: &McpServerConfig,
    policy: McpCredentialPolicy,
) -> Result<(), StartupOutcomeError> {
    if policy != McpCredentialPolicy::ExecutorOnly {
        return Ok(());
    }
    let valid_transport = match &config.transport {
        McpServerTransportConfig::Stdio { cwd, env_vars, .. } => {
            cwd.is_some() && env_vars.iter().all(McpServerEnvVar::is_remote_source)
        }
        McpServerTransportConfig::StreamableHttp {
            env_http_headers,
            http_headers_helper,
            ..
        } => {
            http_headers_helper.is_none() && env_http_headers.as_ref().is_none_or(HashMap::is_empty)
        }
    };
    if config.is_local_environment() || !valid_transport {
        return Err(StartupOutcomeError::from(anyhow!(
            "executor-discovered MCP server '{server_name}' requires executor-owned stdio paths and variables, or HTTP without host environment headers or helpers"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn executor_stdio_accepts_only_normalized_remote_credentials() {
        for (variables, expected) in [
            (json!([]), true),
            (json!([{"name": "TOKEN", "source": "remote"}]), true),
            (json!(["TOKEN"]), false),
            (json!([{"name": "TOKEN", "source": "local"}]), false),
        ] {
            let mut config: McpServerConfig = serde_json::from_value(json!({
                "command": "server", "cwd": "/executor", "environment_id": "computer",
                "env_vars": variables,
            }))
            .unwrap();
            assert_eq!(
                validate("test", &config, McpCredentialPolicy::ExecutorOnly).is_ok(),
                expected
            );
            config.environment_id = "local".to_string();
            assert!(validate("test", &config, McpCredentialPolicy::ExecutorOnly).is_err());
        }
    }

    #[test]
    fn executor_http_still_rejects_host_helpers_and_environment_headers() {
        for extra in [
            json!({"env_http_headers": {"X-Key": "TOKEN"}}),
            json!({"http_headers_helper": "helper"}),
        ] {
            let mut declaration = json!({"url": "http://example.invalid/mcp"});
            declaration
                .as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            let mut config: McpServerConfig = serde_json::from_value(declaration).unwrap();
            config.environment_id = "computer".to_string();
            assert!(validate("test", &config, McpCredentialPolicy::ExecutorOnly).is_err());
        }
    }
}
