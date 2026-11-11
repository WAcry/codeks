// codeks 0022: project fresh executor user settings onto the cached standalone skill inventory.
use std::io;
use std::sync::Arc;

use codex_config::CONFIG_TOML_FILE;
use codex_config::SkillConfigRules;
use codex_config::SkillsConfig;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_absolute_path::AbsolutePathBufGuard;
use codex_utils_home_dir::find_codex_home;
use codex_utils_path_uri::PathUri;

use crate::DiscoverV2CapabilitiesResponse;

pub(crate) async fn apply_user_skill_config(
    inventory: Arc<DiscoverV2CapabilitiesResponse>,
) -> io::Result<Arc<DiscoverV2CapabilitiesResponse>> {
    let codex_home = find_codex_home()?;
    apply_user_skill_config_in_home(inventory, &codex_home).await
}

async fn apply_user_skill_config_in_home(
    inventory: Arc<DiscoverV2CapabilitiesResponse>,
    codex_home: &AbsolutePathBuf,
) -> io::Result<Arc<DiscoverV2CapabilitiesResponse>> {
    let config_path = codex_home.join(CONFIG_TOML_FILE);
    let config = match tokio::fs::read_to_string(&config_path).await {
        Ok(contents) => {
            let value = toml::from_str::<toml::Value>(&contents)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            match value.get("skills") {
                Some(skills) => {
                    // Resolve relative and home paths on the executor, never on the controller.
                    let _base = AbsolutePathBufGuard::new(codex_home.as_path());
                    SkillsConfig::try_from(skills.clone())
                        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                }
                None => SkillsConfig::default(),
            }
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => SkillsConfig::default(),
        Err(error) => return Err(error),
    };
    let rules = SkillConfigRules::from_user_config(&config);
    let mut response = (*inventory).clone();
    response.disabled_skill_paths = Some(if rules.entries.is_empty() {
        Vec::new()
    } else {
        let canonical = inventory
            .skills
            .iter()
            .map(|skill| {
                let path = skill.path.to_abs_path()?;
                Ok(PathUri::from_abs_path(&path.canonicalize().unwrap_or(path)))
            })
            .collect::<io::Result<Vec<_>>>()?;
        let disabled = rules.resolve_disabled_paths(
            inventory
                .skills
                .iter()
                .zip(&canonical)
                .map(|(skill, path)| (skill.name.as_str(), path)),
        );
        inventory
            .skills
            .iter()
            .zip(canonical)
            .filter(|(_, path)| disabled.contains(path))
            .map(|(skill, _)| skill.path.clone())
            .collect()
    });
    Ok(Arc::new(response))
}

#[cfg(test)]
#[path = "executor_skill_config_tests.rs"]
mod tests;
