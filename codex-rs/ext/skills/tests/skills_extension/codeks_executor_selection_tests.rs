// codeks 0023: explicit executor selections preserve native deduplication and provenance.
use super::*;

#[tokio::test]
async fn executor_mentions_keep_one_native_fragment_and_execution_source() -> TestResult {
    for hidden in [false, true] {
        let home = tempfile::tempdir()?;
        let host_path = home.path().join("SKILL.md");
        let host_contents = "Same-name host skill instructions.";
        std::fs::write(&host_path, host_contents)?;
        let host_path = PathUri::from_host_native_path(&host_path)?;
        let mut host_outcome = SkillLoadOutcome::default();
        host_outcome.skills.push(SkillMetadata {
            name: "lint-fix".to_string(),
            description: "Use the host formatter.".to_string(),
            short_description: None,
            interface: None,
            dependencies: None,
            policy: None,
            path_to_skills_md: host_path.clone(),
            scope: SkillScope::User,
            plugin_id: None,
            remote_plugin_id: None,
        });

        let package = "skill://computer-skills/remote-skills/lint-fix";
        let resource = format!("{package}/SKILL.md");
        let remote_path = PathUri::parse("file:///remote-skills/lint-fix/SKILL.md")?;
        let mut entry = SkillCatalogEntry::new(
            SkillPackageId(package.to_string()),
            SkillAuthority::new(SkillSourceKind::Executor, "computer-skills"),
            "lint-fix",
            "Use the computer formatter.",
            SkillResourceId::environment(&resource, "computer", remote_path),
        )
        .with_display_path(&resource);
        if hidden {
            entry = entry.hidden_from_prompt();
        }
        let read_requests = Arc::new(Mutex::new(Vec::new()));
        let providers = SkillProviders::new()
            .with_host_provider(Arc::new(HostSkillProvider::new()))
            .with_executor_provider(Arc::new(StaticSkillProvider {
                catalog: SkillCatalog {
                    entries: vec![entry],
                    ..Default::default()
                },
                read_requests: Arc::clone(&read_requests),
                list_calls: None,
                fail_first_list: false,
            }));
        let mut builder = ExtensionRegistryBuilder::new();
        install_with_providers(&mut builder, providers, skills_extension_config);
        let registry = builder.build();
        let session_store = ExtensionData::new("session");
        let thread_store = ExtensionData::new("thread");
        registry.thread_lifecycle_contributors()[0]
            .on_thread_start(ThreadStartInput {
                config: &default_config(),
                session_source: &SessionSource::Cli,
                persistent_thread_state_available: true,
                environments: &[],
                mcp_resource_client: None,
                extension_metrics: None,
                session_store: &session_store,
                thread_store: &thread_store,
            })
            .await;

        let turn_store = ExtensionData::new("turn");
        turn_store.insert(HostSkillsSnapshot::new(Arc::new(host_outcome)));
        let step_store = ExtensionData::new("step");
        step_store.insert(Vec::<ExecutorCapabilityDiscoveryV2Snapshot>::new());
        let mention = UserInput::Mention {
            name: "lint-fix".to_string(),
            path: resource.clone(),
        };
        let access = FileSystemEnvironmentAccessor::unrestricted(&LOCAL_FS);
        let fragments = prepared_turn_input(
            &registry,
            TurnInputContext {
                step_store: &step_store,
                turn_id: "turn".to_string(),
                user_input: vec![
                    UserInput::Text {
                        text: "Use $lint-fix.".to_string(),
                        text_elements: Vec::new(),
                    },
                    mention.clone(),
                    mention,
                ],
                environments: vec![codex_extension_api::TurnInputEnvironment {
                    environment_id: "computer".to_string(),
                    cwd: PathUri::parse("file:///workspace")?,
                    is_primary: false,
                    fs: &access,
                }],
            },
            None,
            &session_store,
            &thread_store,
            &turn_store,
        )
        .await;

        let selected = fragments
            .iter()
            .filter(|fragment| fragment.role() == "user")
            .collect::<Vec<_>>();
        assert_eq!(selected.len(), 1);
        assert_eq!(
            selected[0].content_kind().0,
            "skills.selected_skill_instructions"
        );
        assert_eq!(
            selected[0].content_metadata(),
            codex_protocol::models::ContentItemMetadata::skills()
        );
        let text = selected[0].render().expect_text();
        assert!(text.starts_with("<skill>"));
        assert!(text.ends_with("</skill>"));
        assert!(text.contains("# Lint Fix\n\nRun the formatter."));
        assert!(!text.contains(host_contents));
        let context = text
            .split_once("<execution_context>")
            .and_then(|(_, text)| text.split_once("</execution_context>"))
            .ok_or("executor skill execution context is missing")?
            .0;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(context)?,
            serde_json::json!({
                "environment_id": "computer",
                "skill_root": "/remote-skills/lint-fix",
            })
        );
        assert_eq!(text.contains("<resource_access>"), hidden);
        assert_eq!(read_request_keys(&read_requests).len(), 1);
        assert!(
            turn_store
                .get::<InjectedHostSkillPrompts>()
                .ok_or("host duplicate suppression marker is missing")?
                .is_superseded_path(&host_path.inferred_native_path_string())
        );
    }
    Ok(())
}

#[tokio::test]
async fn selected_standalone_executor_skill_keeps_the_complete_body() -> TestResult {
    let home = tempfile::tempdir()?;
    let skill_dir = home.path().join("long-guide");
    std::fs::create_dir_all(&skill_dir)?;
    let contents = format!(
        "# Long Guide\n\n{}\nFinal instruction after the former preview limit.\n",
        "Read the next step before changing files.\n".repeat(300),
    );
    let skill_path = skill_dir.join("SKILL.md");
    std::fs::write(&skill_path, &contents)?;
    let snapshots = vec![ExecutorCapabilityDiscoveryV2Snapshot {
        environment_id: "computer".to_string(),
        result: Ok(Arc::new(DiscoverV2CapabilitiesResponse {
            skills: vec![ExecutorSkill {
                name: "long-guide".to_string(),
                description: "Follow the complete guide.".to_string(),
                short_description: None,
                path: PathUri::from_host_native_path(&skill_path)?,
                discovery_root: Some(SkillDiscoveryRoot {
                    id: "computer-skills".to_string(),
                    path: PathUri::from_host_native_path(home.path())?,
                }),
                dependencies: None,
                policy: None,
            }],
            ..Default::default()
        })),
    }];
    let provider = Arc::new(ExecutorSkillProvider::new_with_restriction_product(
        Arc::new(EnvironmentManager::default_for_tests()),
        None,
    ));
    let catalog = provider
        .list(SkillListQuery {
            turn_id: "turn".to_string(),
            executor_roots: Vec::new(),
            resolved_executor_roots: Vec::new(),
            host_snapshot: None,
            include_host_skills: false,
            include_bundled_skills: false,
            include_cloud_skills: false,
            mcp_resources: None,
            executor_capability_discovery: None,
            executor_capability_discovery_v2: Some(snapshots.clone()),
        })
        .await?;
    let selector = catalog.entries[0].main_prompt.as_str().to_string();
    let mut builder = ExtensionRegistryBuilder::new();
    install_with_providers(
        &mut builder,
        SkillProviders::new().with_executor_provider(provider),
        skills_extension_config,
    );
    let registry = builder.build();
    let session_store = ExtensionData::new("session");
    let thread_store = ExtensionData::new("thread");
    registry.thread_lifecycle_contributors()[0]
        .on_thread_start(ThreadStartInput {
            config: &default_config(),
            session_source: &SessionSource::Cli,
            persistent_thread_state_available: true,
            environments: &[],
            mcp_resource_client: None,
            extension_metrics: None,
            session_store: &session_store,
            thread_store: &thread_store,
        })
        .await;
    let turn_store = ExtensionData::new("turn");
    let step_store = ExtensionData::new("step");
    step_store.insert(snapshots);
    let access = FileSystemEnvironmentAccessor::unrestricted(&LOCAL_FS);
    let fragments = prepared_turn_input(
        &registry,
        TurnInputContext {
            step_store: &step_store,
            turn_id: "turn".to_string(),
            user_input: vec![UserInput::Mention {
                name: "long-guide".to_string(),
                path: selector,
            }],
            environments: vec![codex_extension_api::TurnInputEnvironment {
                environment_id: "computer".to_string(),
                cwd: PathUri::from_host_native_path(home.path())?,
                is_primary: false,
                fs: &access,
            }],
        },
        None,
        &session_store,
        &thread_store,
        &turn_store,
    )
    .await;
    let selected = fragments
        .iter()
        .filter(|fragment| fragment.role() == "user")
        .collect::<Vec<_>>();
    assert_eq!(selected.len(), 1);
    assert!(contents.len() > 8_000);
    assert!(selected[0].render().expect_text().contains(&contents));
    Ok(())
}
