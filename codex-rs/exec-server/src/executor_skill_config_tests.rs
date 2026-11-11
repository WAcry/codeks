// codeks 0022: user skill switches refresh independently of cached discovery metadata.
use super::*;
use crate::protocol::CapabilityTextFile;
use crate::protocol::DiscoveredPluginFiles;
use crate::protocol::ExecutorPlugin;
use crate::protocol::ExecutorSkill;
use pretty_assertions::assert_eq;

fn skill(name: &str, path: &std::path::Path) -> io::Result<ExecutorSkill> {
    Ok(ExecutorSkill {
        name: name.to_string(),
        description: "Test skill".to_string(),
        short_description: None,
        path: PathUri::from_host_native_path(path)?,
        discovery_root: None,
        dependencies: None,
        policy: None,
    })
}

#[tokio::test]
async fn user_rules_refresh_cached_inventory_and_preserve_plugin_skills() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let home = AbsolutePathBuf::from_absolute_path(temp.path().join("codex"))?;
    let local_path = home.join("skills/local/SKILL.md");
    let shared_path = temp.path().join("user/.agents/skills/shared/SKILL.md");
    let plugin_path = home.join("plugins/example/SKILL.md");
    for path in [
        local_path.as_path(),
        shared_path.as_path(),
        plugin_path.as_path(),
    ] {
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, "Test instructions")?;
    }
    let standalone = vec![
        skill("review", &local_path)?,
        skill("review", &shared_path)?,
    ];
    let shared_uri = standalone[1].path.clone();
    let inventory = Arc::new(DiscoverV2CapabilitiesResponse {
        plugins: vec![ExecutorPlugin {
            id: "example@test".to_string(),
            display_name: "Example".to_string(),
            remote_plugin_id: None,
            version: "local".to_string(),
            root: PathUri::from_host_native_path(plugin_path.parent().unwrap())?,
            files: DiscoveredPluginFiles {
                manifest: CapabilityTextFile {
                    path: PathUri::from_host_native_path(home.join("plugins/example/plugin.json"))?,
                    contents: "{\"name\":\"example\"}".to_string(),
                },
                mcp_config: None,
                apps_config: None,
            },
            skills: vec![skill("review", &plugin_path)?],
        }],
        skills: standalone,
        ..Default::default()
    });
    std::fs::write(
        home.join(CONFIG_TOML_FILE),
        "[[skills.config]]\nname = 'review'\nenabled = false\n\n\
         [[skills.config]]\npath = 'skills/local/SKILL.md'\nenabled = true\n",
    )?;
    let disabled = apply_user_skill_config_in_home(Arc::clone(&inventory), &home).await?;
    assert_eq!(disabled.disabled_skill_paths, Some(vec![shared_uri]));
    assert_eq!(disabled.skills, inventory.skills);
    assert_eq!(disabled.plugins, inventory.plugins);
    assert_eq!(inventory.disabled_skill_paths, None);

    std::fs::write(
        home.join(CONFIG_TOML_FILE),
        "[[skills.config]]\nname = 'review'\nenabled = true\n",
    )?;
    let enabled = apply_user_skill_config_in_home(Arc::clone(&inventory), &home).await?;
    assert_eq!(enabled.disabled_skill_paths, Some(Vec::new()));
    assert_eq!(disabled.disabled_skill_paths.as_ref().unwrap().len(), 1);
    Ok(())
}

#[tokio::test]
async fn missing_user_config_reports_support_and_old_wire_defaults_to_unsupported()
-> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let home = AbsolutePathBuf::from_absolute_path(temp.path())?;
    let legacy: DiscoverV2CapabilitiesResponse = serde_json::from_value(serde_json::json!({
        "plugins": [], "skills": [], "warnings": [],
    }))?;
    assert_eq!(legacy.disabled_skill_paths, None);
    let response = apply_user_skill_config_in_home(Arc::new(legacy), &home).await?;
    assert_eq!(response.disabled_skill_paths, Some(Vec::new()));
    assert_eq!(
        serde_json::to_value(response.as_ref())?["disabledSkillPaths"],
        serde_json::json!([])
    );
    Ok(())
}

#[tokio::test]
async fn invalid_user_config_is_not_treated_as_enabled() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let home = AbsolutePathBuf::from_absolute_path(temp.path())?;
    for contents in [
        "[skills",
        "[[skills.config]]\nname = 'review'\nenabled = 'invalid'\n",
    ] {
        std::fs::write(home.join(CONFIG_TOML_FILE), contents)?;
        let error = apply_user_skill_config_in_home(
            Arc::new(DiscoverV2CapabilitiesResponse::default()),
            &home,
        )
        .await
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn path_rules_match_symlinks_and_keep_the_discovered_path() -> anyhow::Result<()> {
    let temp = tempfile::tempdir()?;
    let home = AbsolutePathBuf::from_absolute_path(temp.path().join("codex"))?;
    let target = temp.path().join("shared/review");
    std::fs::create_dir_all(home.join("skills"))?;
    std::fs::create_dir_all(&target)?;
    std::fs::write(target.join("SKILL.md"), "Shared instructions")?;
    std::os::unix::fs::symlink(&target, home.join("skills/linked"))?;
    let discovered = skill("review", &home.join("skills/linked/SKILL.md"))?;
    let expected = discovered.path.clone();
    std::fs::write(
        home.join(CONFIG_TOML_FILE),
        "[[skills.config]]\npath = 'skills/linked/SKILL.md'\nenabled = false\n",
    )?;
    let response = apply_user_skill_config_in_home(
        Arc::new(DiscoverV2CapabilitiesResponse {
            skills: vec![discovered],
            ..Default::default()
        }),
        &home,
    )
    .await?;
    assert_eq!(response.disabled_skill_paths, Some(vec![expected]));
    Ok(())
}
