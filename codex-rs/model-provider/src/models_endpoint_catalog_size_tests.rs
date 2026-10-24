// codeks 0012: preserve every model and its metadata in a large HTTP catalog.
use super::*;
use codex_http_client::OutboundProxyPolicy;
use codex_models_manager::bundled_models_response;
use codex_protocol::openai_models::ModelsResponse;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

#[tokio::test]
async fn explicit_catalog_accepts_complete_catalog_above_one_mib() {
    let template = bundled_models_response()
        .expect("bundled catalog should parse")
        .models
        .into_iter()
        .find(|model| model.slug == "gpt-6.1-sol")
        .expect("bundled catalog should contain the modern template");
    let expected = ModelsResponse {
        models: (0..32)
            .map(|index| {
                let mut model = template.clone();
                model.slug = format!("preset-{index}");
                model
            })
            .collect(),
    };
    let body = serde_json::to_vec(&expected).expect("catalog should serialize");
    assert!(body.len() > 1024 * 1024);
    assert!(body.len() < MAX_MODEL_CATALOG_BYTES);
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/codex/models"))
        .respond_with(ResponseTemplate::new(200).set_body_bytes(body))
        .expect(1)
        .mount(&server)
        .await;
    let endpoint = OpenAiModelsEndpoint::new(
        ModelProviderInfo {
            model_catalog_url: Some(format!("{}/codex/models", server.uri()).into()),
            ..ModelProviderInfo::default()
        },
        /*auth_manager*/ None,
        /*gateway_auth_manager*/ None,
    );
    let actual = endpoint
        .list_models(
            "1.2.3",
            HttpClientFactory::new(OutboundProxyPolicy::ReqwestDefault),
        )
        .await
        .expect("complete catalog above one MiB should load");
    assert_eq!(actual.models, expected.models);
}
