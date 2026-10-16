// codeks 0006: application requirements do not depend on default managed sources.
use super::load_local_application_requirements;
use crate::CloudRequirementsTomlBundle;
use crate::LoaderOverrides;
use crate::loader::tests::TestFileSystem;
use pretty_assertions::assert_eq;

#[tokio::test]
async fn default_overrides_load_no_local_application_requirements() {
    let local = load_local_application_requirements(&TestFileSystem, &LoaderOverrides::default())
        .await
        .expect("load application requirements without managed sources");
    assert_eq!(
        local
            .compose(CloudRequirementsTomlBundle::default())
            .unwrap(),
        None
    );
}

#[cfg(target_os = "macos")]
#[tokio::test]
async fn explicit_mdm_override_still_loads_application_requirements() {
    use base64::Engine;
    use base64::prelude::BASE64_STANDARD;

    let contents = "[application.network]\nenabled = false";
    let overrides = LoaderOverrides {
        macos_managed_config_requirements_base64: Some(BASE64_STANDARD.encode(contents)),
        ..Default::default()
    };
    let local = load_local_application_requirements(&TestFileSystem, &overrides)
        .await
        .expect("load explicit MDM application requirements");
    let expected: crate::ConfigRequirementsToml = toml::from_str(contents).unwrap();
    assert_eq!(
        local
            .compose(CloudRequirementsTomlBundle::default())
            .unwrap(),
        expected.application
    );
}
