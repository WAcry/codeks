// codeks 0020: bind ordinary executor MCP stdio configuration to its owning filesystem.

use codex_config::McpServerConfig;
use codex_config::McpServerTransportConfig;
use codex_config::types::McpServerEnvVar;
use codex_utils_path_uri::PathUri;

pub(crate) fn normalize_stdio(
    server: &mut McpServerConfig,
    selected_cwd: &PathUri,
    user_home: Option<&PathUri>,
) -> Result<(), String> {
    // The caller ignores declarations that explicitly target another environment.
    if !server.is_local_environment() {
        return Ok(());
    }
    let McpServerTransportConfig::Stdio { cwd, env_vars, .. } = &mut server.transport else {
        return Ok(());
    };
    let resolved_cwd = match cwd.as_ref() {
        Some(cwd) => PathUri::parse(cwd.as_str())
            .map_err(|error| error.to_string())
            .or_else(|_| {
                cwd.resolve_against(selected_cwd, user_home)
                    .map_err(|error| error.to_string())
            })?,
        None => selected_cwd.clone(),
    };
    *cwd = Some(resolved_cwd.into());
    for variable in env_vars {
        if variable.source() == Some("local") {
            return Err(format!(
                "env_vars entry `{}` cannot read host variables from executor-owned configuration",
                variable.name()
            ));
        }
        *variable = McpServerEnvVar::Config {
            name: variable.name().to_string(),
            source: Some("remote".to_string()),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn stdio_paths_and_credentials_use_the_executor_context() {
        let selected_cwd = PathUri::parse("file:///C:/work/project").unwrap();
        let home = PathUri::parse("file:///C:/Users/example").unwrap();
        for (cwd, expected) in [
            (None, "file:///C:/work/project"),
            (Some(r"..\server"), "file:///C:/work/server"),
            (Some(r"D:\tools"), "file:///D:/tools"),
            (Some("~/tools"), "file:///C:/Users/example/tools"),
            (Some("file:///C:/explicit"), "file:///C:/explicit"),
        ] {
            let mut server: McpServerConfig = serde_json::from_value(json!({
                "command": "server",
                "cwd": cwd,
                "env": {"LITERAL": "configured"},
                "env_vars": ["TOKEN", {"name": "OTHER"}, {"name": "REMOTE", "source": "remote"}],
            }))
            .unwrap();
            normalize_stdio(&mut server, &selected_cwd, Some(&home)).unwrap();
            let McpServerTransportConfig::Stdio {
                cwd, env_vars, env, ..
            } = server.transport
            else {
                panic!("expected stdio");
            };
            assert_eq!(
                PathUri::try_from(cwd.unwrap()).unwrap().to_string(),
                expected
            );
            assert!(env_vars.iter().all(McpServerEnvVar::is_remote_source));
            assert_eq!(
                env.unwrap().get("LITERAL").map(String::as_str),
                Some("configured")
            );
        }
    }

    #[test]
    fn executor_config_cannot_forward_host_environment_variables() {
        let mut server: McpServerConfig = serde_json::from_value(json!({
            "command": "server",
            "env_vars": [{"name": "TOKEN", "source": "local"}],
        }))
        .unwrap();
        let cwd = PathUri::parse("file:///work").unwrap();
        assert!(
            normalize_stdio(&mut server, &cwd, None)
                .unwrap_err()
                .contains("cannot read host variables")
        );
    }

    #[test]
    fn declarations_targeting_another_environment_are_left_for_the_selection_filter() {
        let mut server: McpServerConfig = serde_json::from_value(json!({
            "command": "server", "environment_id": "other", "cwd": "relative",
        }))
        .unwrap();
        let original = server.clone();
        normalize_stdio(&mut server, &PathUri::parse("file:///work").unwrap(), None).unwrap();
        assert_eq!(server, original);
    }
}
