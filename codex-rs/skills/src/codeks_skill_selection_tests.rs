// codeks 0023: an unavailable structured skill selection never retargets a host namesake.
use super::*;
use codex_utils_absolute_path::test_support::PathBufExt;
use codex_utils_absolute_path::test_support::test_path_buf;
use pretty_assertions::assert_eq;

struct HostSkills {
    skills: Vec<SkillMetadata>,
    disabled: HashSet<PathUri>,
}

impl ExplicitSkillLookup for HostSkills {
    fn skills(&self) -> &[SkillMetadata] {
        &self.skills
    }

    fn disabled_paths(&self) -> &HashSet<PathUri> {
        &self.disabled
    }

    fn skill_discovery_path_for_path(&self, _path: &PathUri) -> Option<&PathUri> {
        None
    }
}

#[test]
fn unavailable_structured_skill_mentions_block_only_their_host_namesake() {
    let make_skill = |name: &str| SkillMetadata {
        name: name.to_string(),
        description: "Host skill".to_string(),
        short_description: None,
        interface: None,
        dependencies: None,
        policy: None,
        path_to_skills_md: test_path_buf(&format!("/skills/{name}/SKILL.md"))
            .abs()
            .into(),
        scope: codex_protocol::protocol::SkillScope::User,
        plugin_id: None,
        remote_plugin_id: None,
    };
    let review = make_skill("review");
    let build = make_skill("build");
    let host = HostSkills {
        skills: vec![review.clone(), build.clone()],
        disabled: HashSet::new(),
    };
    for path in [
        "skill://computer-skills/shared/review/SKILL.md",
        "skill://computer-skills/C:/skills/review/SKILL.md",
    ] {
        let selected = collect_explicit_skill_mentions(
            &[
                UserInput::Text {
                    text: "Use $review and $build.".to_string(),
                    text_elements: Vec::new(),
                },
                UserInput::Mention {
                    name: "review".to_string(),
                    path: path.to_string(),
                },
            ],
            &host,
            &HashMap::new(),
        );
        assert_eq!(selected, vec![build.clone()]);
    }

    for path in [
        review.path_to_skills_md.inferred_native_path_string(),
        "app://review".to_string(),
    ] {
        let selected = collect_explicit_skill_mentions(
            &[
                UserInput::Text {
                    text: "Use $review.".to_string(),
                    text_elements: Vec::new(),
                },
                UserInput::Mention {
                    name: "review".to_string(),
                    path,
                },
            ],
            &host,
            &HashMap::new(),
        );
        assert_eq!(selected, vec![review.clone()]);
    }
}
