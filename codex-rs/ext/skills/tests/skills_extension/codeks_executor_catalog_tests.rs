// codeks 0022: runtime and external directories share executor-owned skill policy.
use anyhow::Result;
use codex_exec_server::DiscoverV2CapabilitiesResponse;
use codex_exec_server::EnvironmentManager;
use codex_exec_server::ExecutorCapabilityDiscoveryV2Snapshot;
use codex_skills_extension::ExecutorSkillProvider;
use codex_skills_extension::provider::SkillListQuery;
use codex_skills_extension::provider::SkillProvider;
use serde_json::json;
use std::sync::Arc;

#[tokio::test]
async fn executor_disable_rules_are_environment_scoped_and_keep_explicit_only_entries() -> Result<()>
{
    let path = "file:///home/test/.agents/skills/review/SKILL.md";
    let inventory = |disabled: Option<Vec<&str>>| -> Result<_> {
        Ok(Arc::new(serde_json::from_value::<
            DiscoverV2CapabilitiesResponse,
        >(json!({
            "plugins": [],
            "skills": [{
                "name": "review", "description": "Review changes.", "path": path,
                "discoveryRoot": {"id": "user-agent-skills", "path": "file:///home/test/.agents/skills"},
                "dependencies": null, "policy": {"allowImplicitInvocation": false, "products": []}
            }],
            "disabledSkillPaths": disabled,
            "warnings": []
        }))?))
    };
    let provider = ExecutorSkillProvider::new_with_restriction_product(
        Arc::new(EnvironmentManager::default_for_tests()),
        None,
    );
    let snapshots = vec![
        ExecutorCapabilityDiscoveryV2Snapshot {
            environment_id: "first".into(),
            result: Ok(inventory(Some(vec![path]))?),
        },
        ExecutorCapabilityDiscoveryV2Snapshot {
            environment_id: "second".into(),
            result: Ok(inventory(None)?),
        },
    ];
    let catalog = provider
        .list(SkillListQuery {
            turn_id: "turn-1".to_string(),
            executor_roots: Vec::new(),
            resolved_executor_roots: Vec::new(),
            host_snapshot: None,
            include_host_skills: false,
            include_bundled_skills: false,
            include_cloud_skills: false,
            mcp_resources: None,
            executor_capability_discovery: None,
            executor_capability_discovery_v2: Some(snapshots),
        })
        .await?;
    assert_eq!(catalog.entries.len(), 2);
    let first = &catalog.entries[0];
    let second = &catalog.entries[1];
    assert!(!first.enabled);
    assert!(second.enabled);
    assert!(!first.prompt_visible);
    assert!(!second.prompt_visible);
    assert_eq!(first.main_prompt.environment_id(), Some("first"));
    assert_eq!(second.main_prompt.environment_id(), Some("second"));
    // The native locator remains valid even for entries omitted from the automatic prompt.
    assert_eq!(
        second.main_prompt.as_str(),
        "skill://user-agent-skills/home/test/.agents/skills/review/SKILL.md"
    );
    Ok(())
}
