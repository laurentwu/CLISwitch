use std::collections::BTreeMap;
use std::path::Path;

use chrono::Utc;
use cliswitch_lib::adapters::{CliAdapter, HostEnvironment, OpenCodeAdapter};
use cliswitch_lib::catalog::{
    ProviderCatalog, install_runtime_catalog, legacy_catalog, runtime_catalog,
};
use cliswitch_lib::domain::{
    ApiProviderData, CliId, ConfigurationTarget, ProviderConnection, ProviderData, ProviderProfile,
    VerificationInfo,
};
use cliswitch_lib::services::config_writer::parse_jsonc_value;
use tempfile::TempDir;
use url::Url;
use uuid::Uuid;

// Runs in a dedicated test binary: it swaps the process-global runtime catalog, which must not
// be visible to any other adapter test.
fn environment(home: &Path) -> HostEnvironment {
    HostEnvironment {
        home: home.to_path_buf(),
        variables: BTreeMap::new(),
        present_variables: Default::default(),
        os: std::env::consts::OS.into(),
    }
}

struct CatalogRestore(ProviderCatalog);
impl Drop for CatalogRestore {
    fn drop(&mut self) {
        install_runtime_catalog(self.0.clone());
    }
}

// Both tests swap the same process-global catalog; they must not interleave. The async-aware
// mutex may be held across the awaited adapter calls.
static CATALOG_SWAP_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn catalog_swap_guard() -> tokio::sync::MutexGuard<'static, ()> {
    CATALOG_SWAP_LOCK.lock().await
}

fn legacy_go_profile() -> (ProviderProfile, Uuid) {
    let legacy = legacy_catalog().unwrap();
    let template = legacy.api_template("opencode-go").unwrap();
    let connection_id = Uuid::new_v4();
    let connections = template
        .endpoints
        .iter()
        .map(|endpoint| ProviderConnection {
            id: if endpoint.id == "chat" {
                connection_id
            } else {
                Uuid::new_v4()
            },
            template_endpoint_id: Some(endpoint.id.clone()),
            credential_slot_id: endpoint.credential_slot_id.clone(),
            protocol: endpoint.protocol,
            endpoint: endpoint.base_url.clone(),
            auth_type: endpoint.default_auth_type().unwrap(),
            api_key: "fixture-go-key".into(),
            default_model: endpoint
                .models
                .first()
                .map(|model| model.id.clone())
                .unwrap_or_else(|| "glm-5.3".into()),
            verification: VerificationInfo::default(),
        })
        .collect::<Vec<_>>();
    (
        ProviderProfile {
            id: Uuid::new_v4(),
            name: "Legacy OpenCode Go".into(),
            template_id: Some("opencode-go".into()),
            revision: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            data: ProviderData::Api(ApiProviderData { connections }),
        },
        connection_id,
    )
}

async fn write_fixture(path: &Path, content: &str) {
    tokio::fs::create_dir_all(path.parent().unwrap())
        .await
        .unwrap();
    tokio::fs::write(path, content).await.unwrap();
}

// A runtime catalog that no longer lists zhipuai-coding-plan must not reinterpret its native
// entries through the legacy glm-coding-plan identity: the fixed contract keeps the native
// template identity, endpoint, and auth, and fabricates no template endpoint ID.
#[tokio::test]
async fn dropped_catalog_entries_keep_the_native_identity() {
    let _swap_guard = catalog_swap_guard().await;
    let original = runtime_catalog().unwrap();
    let _restore = CatalogRestore(original.clone());
    let mut stripped = original;
    stripped
        .provider_templates
        .retain(|template| template.id() != "zhipuai-coding-plan");
    stripped.relations.retain(|relation| {
        relation.provider_template_id() != "zhipuai-coding-plan"
            || relation.cli_id() != CliId::Opencode
    });
    if let Some(infos) = stripped.provider_info.as_mut() {
        infos.retain(|info| info.id != "zhipuai-coding-plan");
    }
    install_runtime_catalog(stripped);

    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let host = environment(temp.path());
    let paths = adapter.resolve_paths(&host, None);
    write_fixture(
        &paths.config_file,
        r#"{ "model": "zhipuai-coding-plan/fixture-model" }"#,
    )
    .await;
    write_fixture(
        paths.auth_file.as_ref().unwrap(),
        r#"{ "zhipuai-coding-plan": { "type": "api", "key": "fixture-native-key" } }"#,
    )
    .await;

    let read = adapter.read_current(&paths, &host).await.unwrap();
    let candidate = read
        .unmanaged_api_candidates
        .iter()
        .find(|candidate| candidate.source_provider_id == "zhipuai-coding-plan")
        .unwrap();
    assert_eq!(
        candidate.template_id.as_deref(),
        Some("zhipuai-coding-plan")
    );
    assert!(!candidate.model_routed);
    assert_eq!(
        candidate.connection.protocol,
        cliswitch_lib::domain::CliProtocol::OpenaiChat
    );
    assert_eq!(
        candidate.connection.endpoint.as_str(),
        "https://open.bigmodel.cn/api/coding/paas/v4"
    );
    assert_eq!(
        candidate.connection.auth_type,
        cliswitch_lib::domain::ConnectionAuthType::Bearer
    );
    // The legacy fallback template is not this provider's identity, so no endpoint ID is
    // invented for it.
    assert_eq!(candidate.connection.template_endpoint_id, None);
}

// A validated legacy opencode-go record whose chat connection matches the fixed native
// contract applies natively even when the target model has no route in the old static catalog.
// The same record with a custom chat address stays Generic and keeps the old model-routing
// constraint for unroutable model IDs.
#[tokio::test]
async fn legacy_opencode_go_records_stay_native_and_keep_generic_route_constraints() {
    let _swap_guard = catalog_swap_guard().await;
    let original = runtime_catalog().unwrap();
    let _restore = CatalogRestore(original.clone());
    // Force the legacy catalog fallback for opencode-go by dropping the dynamic provider.
    let mut stripped = original;
    stripped
        .provider_templates
        .retain(|template| template.id() != "opencode-go");
    stripped.relations.retain(|relation| {
        relation.provider_template_id() != "opencode-go" || relation.cli_id() != CliId::Opencode
    });
    if let Some(infos) = stripped.provider_info.as_mut() {
        infos.retain(|info| info.id != "opencode-go");
    }
    install_runtime_catalog(stripped);

    let (provider, connection_id) = legacy_go_profile();
    provider.validate().unwrap();

    let temp = TempDir::new().unwrap();
    let adapter = OpenCodeAdapter;
    let paths = adapter.resolve_paths(&environment(temp.path()), None);
    write_fixture(&paths.config_file, "{}\n").await;
    write_fixture(paths.auth_file.as_ref().unwrap(), "{}\n").await;

    let native_target = ConfigurationTarget::Api {
        cli_id: CliId::Opencode,
        provider_id: provider.id,
        connection_id,
        model: "unrouted-native-model".into(),
    };
    let plan = adapter
        .plan_write(&paths, &native_target, &provider, &environment(temp.path()))
        .await
        .unwrap();
    let config =
        parse_jsonc_value(std::str::from_utf8(&plan.files[0].target_content).unwrap()).unwrap();
    assert_eq!(config["model"], "opencode-go/unrouted-native-model");

    let ProviderData::Api(api) = &provider.data else {
        unreachable!()
    };
    let mut custom_connections = api.connections.clone();
    let custom_chat = custom_connections
        .iter_mut()
        .find(|connection| connection.template_endpoint_id.as_deref() == Some("chat"))
        .unwrap();
    custom_chat.endpoint = Url::parse("https://saved-go-endpoint.invalid/v1").unwrap();
    let custom_connection_id = custom_chat.id;
    let mut custom_provider = provider.clone();
    let ProviderData::Api(custom_api) = &mut custom_provider.data else {
        unreachable!()
    };
    custom_api.connections = custom_connections;
    custom_provider.validate().unwrap();

    let unroutable_target = ConfigurationTarget::Api {
        cli_id: CliId::Opencode,
        provider_id: custom_provider.id,
        connection_id: custom_connection_id,
        model: "unrouted-native-model".into(),
    };
    let error = adapter
        .plan_write(
            &paths,
            &unroutable_target,
            &custom_provider,
            &environment(temp.path()),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no route"));

    let routed_target = ConfigurationTarget::Api {
        cli_id: CliId::Opencode,
        provider_id: custom_provider.id,
        connection_id: custom_connection_id,
        model: "glm-5.3".into(),
    };
    let plan = adapter
        .plan_write(
            &paths,
            &routed_target,
            &custom_provider,
            &environment(temp.path()),
        )
        .await
        .unwrap();
    let config_text = std::str::from_utf8(&plan.files[0].target_content).unwrap();
    let config = parse_jsonc_value(config_text).unwrap();
    let generic_id = cliswitch_lib::adapters::namespaced_provider_id(custom_provider.id);
    assert_eq!(config["model"], format!("{generic_id}/glm-5.3"));
    assert_eq!(
        config["provider"][&generic_id]["options"]["baseURL"],
        "https://saved-go-endpoint.invalid/v1"
    );
    assert_eq!(
        config["provider"][&generic_id]["models"]["glm-5.3"]["name"],
        "glm-5.3"
    );
}
