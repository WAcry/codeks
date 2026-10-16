// codeks 0006: supplied ChatGPT tokens must not re-enable implicit account policy.

use super::managed_config_bundle_loader_for_auth;
use codex_config::ConfigRequirementsToml;
use codex_login::CodexAuth;

#[tokio::test]
async fn supplied_tokens_do_not_fetch_implicit_account_policy() {
    let home = tempfile::tempdir().unwrap();
    let mut config = codex_core::config::ConfigBuilder::default()
        .codex_home(home.path().into())
        .build()
        .await
        .unwrap()
        .auth_config();
    // An implicit policy fetch would fail URL validation before issuing a request.
    config.chatgpt_base_url = Some("http://127.0.0.1:1/backend-api/".into());
    let requirements = ConfigRequirementsToml::default();

    for plan in [None, Some("business"), Some("enterprise"), Some("unknown")] {
        let auth =
            CodexAuth::from_external_chatgpt_tokens("e30.e30.c2ln", "account-123", plan).unwrap();
        let loader = managed_config_bundle_loader_for_auth(config.clone(), &auth, &requirements)
            .await
            .unwrap();
        assert!(loader.config_server().is_none());
        assert!(loader.get().await.unwrap().is_none());
    }
}
