// codeks 0019: inspect native skill catalogs without loading or resuming a thread.
use super::CatalogRequestProcessor;
use codex_app_server_protocol::ClientResponsePayload;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::SkillCatalogMetadata;
use codex_app_server_protocol::SkillsCatalogEntry;
use codex_app_server_protocol::SkillsCatalogParams;
use codex_app_server_protocol::SkillsCatalogResponse;
use codex_app_server_protocol::TurnEnvironmentParams;
use codex_exec_server::DiscoverV2CapabilitiesRequest;
use codex_exec_server::ExecutorCapabilityDiscoveryV2Snapshot;
use codex_skills_extension::ExecutorSkillProvider;
use codex_skills_extension::HostSkillsLoadInput;
use codex_skills_extension::catalog::SkillCatalog;
use codex_skills_extension::provider::host_skill_catalog;
use codex_utils_path_uri::LegacyAppPathString;
use codex_utils_path_uri::PathUri;
use futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;

impl CatalogRequestProcessor {
    pub(crate) async fn skills_catalog(
        &self,
        params: SkillsCatalogParams,
    ) -> Result<Option<ClientResponsePayload>, JSONRPCErrorError> {
        let force_reload = params.force_reload;
        let mut entries = futures::stream::iter(params.environments.into_iter().enumerate())
            .map(|(index, selection)| async move {
                let result = self
                    .skill_catalog_for_environment(&selection, force_reload)
                    .await;
                let (skills, errors, supports_skill_config) = match result {
                    Ok((catalog, supported)) => {
                        let skills = catalog
                            .entries
                            .into_iter()
                            .map(|entry| {
                                let path = entry
                                    .main_prompt
                                    .environment_path()
                                    .map(|(_, path)| path.clone().into())
                                    .unwrap_or_else(|| {
                                        LegacyAppPathString::from_string(entry.main_prompt.as_str())
                                    });
                                SkillCatalogMetadata {
                                    name: entry.name,
                                    description: entry.description,
                                    short_description: entry.short_description,
                                    selector: entry.main_prompt.as_str().to_string(),
                                    path,
                                    enabled: entry.enabled,
                                    allow_implicit_invocation: entry.prompt_visible,
                                    plugin_id: entry.plugin_id,
                                }
                            })
                            .collect();
                        (skills, catalog.warnings, supported)
                    }
                    Err(error) => (Vec::new(), vec![error], false),
                };
                (
                    index,
                    SkillsCatalogEntry {
                        environment_id: selection.environment_id,
                        cwd: selection.cwd,
                        skills,
                        supports_skill_config,
                        errors,
                    },
                )
            })
            .buffer_unordered(super::SKILLS_LIST_CWD_CONCURRENCY)
            .collect::<Vec<_>>()
            .await;
        entries.sort_by_key(|(index, _)| *index);
        Ok(Some(
            SkillsCatalogResponse {
                data: entries.into_iter().map(|(_, entry)| entry).collect(),
            }
            .into(),
        ))
    }

    async fn skill_catalog_for_environment(
        &self,
        selection: &TurnEnvironmentParams,
        force_reload: bool,
    ) -> Result<(SkillCatalog, bool), String> {
        let environment_manager = self.thread_manager.environment_manager();
        let environment = environment_manager
            .get_environment(&selection.environment_id)
            .ok_or_else(|| format!("unknown environment id `{}`", selection.environment_id))?;
        let cwd = PathUri::try_from(selection.cwd.clone()).map_err(|error| error.to_string())?;
        if environment.is_remote() {
            // Inventory is metadata only. Runtime applies its current sandbox again when
            // selecting and reading skills; this query never activates a thread or plugin.
            let inventory = tokio::time::timeout(
                Duration::from_secs(5),
                environment
                    .discover_capabilities_v2(DiscoverV2CapabilitiesRequest { cwd, sandbox: None }),
            )
            .await
            .map_err(|_| "executor skill catalog discovery timed out".to_string())?
            .map_err(|error| error.to_string())?;
            let supported = inventory.disabled_skill_paths.is_some();
            let snapshot = ExecutorCapabilityDiscoveryV2Snapshot {
                environment_id: selection.environment_id.clone(),
                result: Ok(Arc::new(inventory)),
            };
            let provider = ExecutorSkillProvider::new_with_restriction_product(
                environment_manager,
                self.thread_manager.session_source().restriction_product(),
            );
            let mut catalog = provider.list_from_discovery_v2(&[snapshot]);
            // Plugin acceptance is thread-owned. This passive directory deliberately
            // covers the executor's standalone user/system skills only.
            catalog.entries.retain(|entry| entry.plugin_id.is_none());
            return Ok((catalog, supported));
        }

        let cwd = cwd.to_abs_path().map_err(|error| error.to_string())?;
        let config = self
            .load_latest_config(Some(cwd.to_path_buf()))
            .await
            .map_err(|error| error.message)?;
        let skills_service = self.thread_manager.skills_service();
        let plugins_manager = self.thread_manager.plugins_manager();
        if force_reload {
            plugins_manager.clear_cache();
            skills_service.clear_cache();
        }
        let plugins_input = config.plugins_config_input();
        let plugins = plugins_manager.plugins_for_config(&plugins_input).await;
        let snapshots = plugins_manager.plugin_skill_snapshots_for_config(&plugins_input);
        let input = HostSkillsLoadInput::new(
            config.cwd.clone(),
            plugins.effective_plugin_skill_roots(),
            config.config_layer_stack,
        )
        .with_plugin_skill_snapshots(snapshots);
        let request = skills_service.for_request();
        let snapshot = request
            .snapshot_for_cwd(&input, force_reload, Some(environment.get_filesystem()))
            .await;
        Ok((host_skill_catalog(snapshot.outcome()), true))
    }
}
