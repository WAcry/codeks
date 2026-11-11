// codeks 0019: inspect selected environments without creating or resuming a thread.
use super::TurnEnvironmentParams;
use crate::JsonSchema;
use crate::TS;
use codex_utils_path_uri::LegacyAppPathString;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SkillsCatalogParams {
    /// The caller's saved or proposed selection. This request does not load a thread.
    pub environments: Vec<TurnEnvironmentParams>,
    /// Refresh host caches. Executors use their native watched capability inventory.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub force_reload: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SkillsCatalogResponse {
    pub data: Vec<SkillsCatalogEntry>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SkillsCatalogEntry {
    pub environment_id: String,
    pub cwd: LegacyAppPathString,
    pub skills: Vec<SkillCatalogMetadata>,
    /// Whether this environment applies user-level skills.config enablement rules.
    pub supports_skill_config: bool,
    /// Discovery errors belong to this environment; other entries remain available.
    pub errors: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct SkillCatalogMetadata {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub short_description: Option<String>,
    /// Pass executor skill:// selectors unchanged in UserInput::Mention.path.
    /// Host paths retain the existing UserInput::Skill.path input contract.
    pub selector: String,
    /// Environment-native SKILL.md path, including for executor-backed entries.
    pub path: LegacyAppPathString,
    pub enabled: bool,
    /// False hides the skill from the automatic model catalog, not from explicit selection.
    pub allow_implicit_invocation: bool,
    pub plugin_id: Option<String>,
}
