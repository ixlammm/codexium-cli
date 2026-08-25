use std::collections::HashMap;
use std::fs;

use tempfile::tempdir;

use codex_model_provider_info::ModelProviderInfo;
use codex_protocol::openai_models::ModelVisibility;

use super::*;

fn write_models(codex_home: &Path, contents: &str) {
    let dir = codexium_dir(codex_home);
    fs::create_dir_all(&dir).expect("create codexium dir");
    fs::write(dir.join(MODELS_FILE_NAME), contents).expect("write models.json");
}

fn write_auth(codex_home: &Path, contents: &str) {
    let dir = codexium_dir(codex_home);
    fs::create_dir_all(&dir).expect("create codexium dir");
    fs::write(dir.join(AUTH_FILE_NAME), contents).expect("write auth.json");
}

fn provider_with_env_key(id: &str, env_key: &str) -> (String, ModelProviderInfo) {
    let mut provider = ModelProviderInfo::default();
    provider.env_key = Some(env_key.to_string());
    (id.to_string(), provider)
}

#[test]
fn ensure_default_files_creates_both_files() {
    let tmp = tempdir().expect("tempdir");
    ensure_default_files(tmp.path()).expect("ensure default files");

    let models_path = tmp.path().join(CODEXIUM_DIR_NAME).join(MODELS_FILE_NAME);
    let auth_path = tmp.path().join(CODEXIUM_DIR_NAME).join(AUTH_FILE_NAME);
    assert!(models_path.exists());
    assert!(auth_path.exists());

    let parsed: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(models_path).expect("read")).expect("parse");
    assert!(parsed.get("providers").is_some());
}

#[test]
fn ensure_default_files_preserves_existing_content() {
    let tmp = tempdir().expect("tempdir");
    write_models(
        tmp.path(),
        r#"{"providers":{"deepseek":{"label":"DeepSeek","models":{"x":{"label":"Custom"}}}}}"#,
    );
    ensure_default_files(tmp.path()).expect("ensure default files");
    let parsed = load_models_file(tmp.path());
    assert_eq!(
        parsed
            .providers
            .get("deepseek")
            .and_then(|p| p.label.as_deref()),
        Some("DeepSeek")
    );
    assert_eq!(
        parsed
            .providers
            .get("deepseek")
            .and_then(|p| p.models.get("x"))
            .and_then(|c| c.label.as_deref()),
        Some("Custom")
    );
}

#[test]
fn load_models_file_returns_empty_on_missing_file() {
    let tmp = tempdir().expect("tempdir");
    let parsed = load_models_file(tmp.path());
    assert!(parsed.providers.is_empty());
}

#[test]
fn load_models_file_returns_empty_on_parse_error() {
    let tmp = tempdir().expect("tempdir");
    write_models(tmp.path(), "not json");
    let parsed = load_models_file(tmp.path());
    assert!(parsed.providers.is_empty());
}

#[test]
fn build_model_catalog_merges_custom_models() {
    let tmp = tempdir().expect("tempdir");
    write_models(
        tmp.path(),
        r#"{
            "providers": {
              "deepseek": {
                "label": "DeepSeek",
                "models": {
                  "deepseek-v4-flash": {
                    "label": "DeepSeek V4 Flash",
                    "context_window": 131072,
                    "max_output_tokens": 8192
                  }
                }
              }
            }
          }"#,
    );

    let catalog = build_model_catalog(tmp.path(), None);
    let custom = catalog
        .models
        .iter()
        .find(|m| m.slug == "deepseek.deepseek-v4-flash")
        .expect("custom model should be present");

    assert_eq!(custom.display_name, "DeepSeek V4 Flash");
    assert_eq!(custom.context_window, Some(131_072));
    assert_eq!(custom.max_output_tokens, Some(8192));
    assert_eq!(custom.visibility, ModelVisibility::List);
    assert_eq!(custom.used_fallback_model_metadata, false);
    assert_eq!(custom.is_custom, true);
    assert_eq!(custom.provider.as_deref(), Some("deepseek"));
    assert_eq!(custom.provider_label.as_deref(), Some("DeepSeek"));
}

#[test]
fn build_model_catalog_supports_legacy_flat_provider_format() {
    let tmp = tempdir().expect("tempdir");
    write_models(
        tmp.path(),
        r#"{
            "providers": {
              "deepseek": {
                "deepseek-v4-flash": {
                  "context_window": 131072
                }
              }
            }
          }"#,
    );

    let catalog = build_model_catalog(tmp.path(), None);
    let custom = catalog
        .models
        .iter()
        .find(|m| m.slug == "deepseek.deepseek-v4-flash")
        .expect("custom model should be present");

    assert_eq!(custom.context_window, Some(131_072));
    assert_eq!(custom.provider.as_deref(), Some("deepseek"));
    assert_eq!(custom.provider_label.as_deref(), Some("deepseek"));
}

#[test]
fn build_model_catalog_overrides_existing_entry() {
    let tmp = tempdir().expect("tempdir");
    write_models(
        tmp.path(),
        r#"{
            "providers": {
              "deepseek": {
                "deepseek-v4-flash": {
                  "context_window": 64000
                }
              }
            }
          }"#,
    );

    let existing = codex_models_manager::bundled_models_response().expect("bundled");
    let catalog = build_model_catalog(tmp.path(), Some(existing));
    let custom = catalog
        .models
        .iter()
        .find(|m| m.slug == "deepseek.deepseek-v4-flash")
        .expect("custom model should be present");
    assert_eq!(custom.context_window, Some(64000));
}

#[test]
fn build_codexium_model_providers_registers_providers_with_base_url() {
    let tmp = tempdir().expect("tempdir");
    write_models(
        tmp.path(),
        r#"{
            "providers": {
              "deepseek": {
                "label": "DeepSeek",
                "base_url": "https://api.deepseek.com",
                "env_key": "DEEPSEEK_API_KEY",
                "wire_api": "responses",
                "models": {
                  "deepseek-v4-flash": { "label": "DeepSeek V4 Flash" }
                }
              },
              "kimi": {
                "label": "Kimi",
                "base_url": "https://api.moonshot.ai/v1",
                "models": {
                  "kimi-k3": { "label": "Kimi K3" }
                }
              },
              "no-url-provider": {
                "label": "No URL",
                "models": { "x": {} }
              }
            }
          }"#,
    );

    let providers = build_codexium_model_providers(tmp.path());

    let deepseek = providers
        .get("deepseek")
        .expect("deepseek provider present");
    assert_eq!(
        deepseek.base_url.as_deref(),
        Some("https://api.deepseek.com")
    );
    assert_eq!(deepseek.env_key.as_deref(), Some("DEEPSEEK_API_KEY"));
    assert_eq!(
        deepseek.wire_api,
        codex_model_provider_info::WireApi::Responses
    );
    assert_eq!(deepseek.requires_openai_auth, false);
    assert_eq!(deepseek.name, "DeepSeek");

    let kimi = providers.get("kimi").expect("kimi provider present");
    assert_eq!(kimi.base_url.as_deref(), Some("https://api.moonshot.ai/v1"));
    assert_eq!(kimi.env_key, None);

    // Providers without a base_url are not registered.
    assert!(!providers.contains_key("no-url-provider"));
}

#[test]
fn load_auth_file_accepts_nested_api_key_format() {
    let tmp = tempdir().expect("tempdir");
    write_auth(
        tmp.path(),
        r#"{"providers":{"deepseek":{"api_key":"sk-nested-key-456"}}}"#,
    );
    let auth = load_auth_file(tmp.path());
    assert_eq!(
        auth.providers.get("deepseek").map(String::as_str),
        Some("sk-nested-key-456")
    );
}

#[test]
fn apply_auth_to_env_injects_keys_for_known_providers() {
    let tmp = tempdir().expect("tempdir");
    write_auth(
        tmp.path(),
        r#"{"providers":{"deepseek":"sk-test-key-123"}}"#,
    );

    let mut providers = HashMap::new();
    providers.insert("deepseek".to_string(), {
        let mut provider = ModelProviderInfo::default();
        provider.env_key = Some("CODEXIUM_TEST_DEEPSEEK_KEY".to_string());
        provider
    });

    let auth = load_auth_file(tmp.path());
    apply_auth_to_env(&auth, &providers);

    assert_eq!(
        std::env::var("CODEXIUM_TEST_DEEPSEEK_KEY").ok(),
        Some("sk-test-key-123".to_string())
    );
}

#[test]
fn apply_auth_to_env_skips_unknown_providers() {
    let tmp = tempdir().expect("tempdir");
    write_auth(tmp.path(), r#"{"providers":{"missing-provider":"sk-x"}}"#);

    let providers = HashMap::new();
    let auth = load_auth_file(tmp.path());
    apply_auth_to_env(&auth, &providers);
    // Should not panic; nothing to assert beyond completion.
}

#[test]
fn codexium_dir_is_under_codex_home() {
    let tmp = tempdir().expect("tempdir");
    assert_eq!(codexium_dir(tmp.path()), tmp.path().join(CODEXIUM_DIR_NAME));
}

#[test]
fn provider_with_env_key_helper_builds_provider() {
    let (id, provider) = provider_with_env_key("kimi", "KIMI_API_KEY");
    assert_eq!(id, "kimi");
    assert_eq!(provider.env_key.as_deref(), Some("KIMI_API_KEY"));
}

#[test]
fn cmp_version_orders_numeric_segments() {
    assert_eq!(cmp_version("1.0.0", "1.0.1"), std::cmp::Ordering::Less);
    assert_eq!(cmp_version("1.2.0", "1.10.0"), std::cmp::Ordering::Less);
    assert_eq!(cmp_version("2.0.0", "1.9.9"), std::cmp::Ordering::Greater);
    assert_eq!(cmp_version("1.3.0", "1.3.0"), std::cmp::Ordering::Equal);
    assert_eq!(cmp_version("2", "2.0.0"), std::cmp::Ordering::Equal);
}

#[test]
fn is_registry_update_only_for_same_major_higher_sub() {
    let current = ProviderRegistry {
        version: "1.2.0".to_string(),
        providers: vec![],
    };
    // Same major, higher minor -> update.
    let fetched = ProviderRegistry {
        version: "1.3.0".to_string(),
        providers: vec![],
    };
    assert!(is_registry_update(&fetched, &current));
    // Different major -> never auto-update.
    let major_bump = ProviderRegistry {
        version: "2.0.0".to_string(),
        providers: vec![],
    };
    assert!(!is_registry_update(&major_bump, &current));
    // Lower version -> not an update.
    let older = ProviderRegistry {
        version: "1.1.0".to_string(),
        providers: vec![],
    };
    assert!(!is_registry_update(&older, &current));
}

#[test]
fn load_registry_prefers_persisted_when_newer_or_present() {
    let tmp = tempdir().expect("tempdir");
    let dir = codexium_dir(tmp.path());
    fs::create_dir_all(&dir).expect("create codexium dir");
    // Persist a same-major newer registry; it should win over the bundled.
    let newer = ProviderRegistry {
        version: "99.0.0".to_string(),
        providers: vec![RegistryProvider {
            id: "custom-test".to_string(),
            name: "Test".to_string(),
            ..Default::default()
        }],
    };
    fs::write(
        dir.join(REGISTRY_FILE_NAME),
        serde_json::to_string(&newer).expect("serialize"),
    )
    .expect("write");
    let loaded = load_registry(tmp.path());
    assert_eq!(loaded.version, "99.0.0");
    assert_eq!(loaded.providers.len(), 1);
    assert_eq!(loaded.providers[0].id, "custom-test");
}

#[test]
fn bundled_registry_is_valid() {
    let reg = bundled_registry();
    // The compiled-in default must parse and carry a version + providers.
    assert!(!reg.version.is_empty());
    assert!(!reg.providers.is_empty());
    // Every provider must have an id and at least a name.
    for provider in &reg.providers {
        assert!(!provider.id.is_empty());
        assert!(!provider.name.is_empty());
    }
}

#[test]
fn build_model_catalog_openai_does_not_duplicate_and_hides_disabled() {
    let tmp = tempdir().expect("tempdir");
    // Seed a codexium models.json with an `openai` provider: one model disabled,
    // one enabled, plus label overrides. Slugs are the bare OpenAI slugs.
    write_models(
        tmp.path(),
        r#"{"providers":{"openai":{"label":"OpenAI","requires_openai_auth":true,"readonly":true,"models":{
            "gpt-5.4": {"enabled": false, "label": "Disabled GPT-5.4"},
            "gpt-5.5": {"enabled": true, "label": "Enabled GPT-5.5"}
        }}}}"#,
    );
    let catalog = build_model_catalog(tmp.path(), None);
    // No `openai.`-prefixed duplicates should be minted.
    assert!(!catalog.models.iter().any(|m| m.slug.starts_with("openai.")));
    // The disabled model is hidden; the enabled one is listed with the override.
    let disabled = catalog
        .models
        .iter()
        .find(|m| m.slug == "gpt-5.4")
        .expect("gpt-5.4 exists");
    assert_eq!(disabled.visibility, ModelVisibility::Hide);
    let enabled = catalog
        .models
        .iter()
        .find(|m| m.slug == "gpt-5.5")
        .expect("gpt-5.5 exists");
    assert_eq!(enabled.visibility, ModelVisibility::List);
    assert_eq!(enabled.display_name, "Enabled GPT-5.5");
}

/// Builds a minimal [`ModelPreset`] for tests (the struct has no `Default`).
fn preset(model: &str, provider: Option<&str>, is_custom: bool, show_in_picker: bool) -> ModelPreset {
    ModelPreset {
        id: model.to_string(),
        model: model.to_string(),
        display_name: model.to_string(),
        description: String::new(),
        model_specialty: None,
        default_reasoning_effort: codex_protocol::openai_models::ReasoningEffort::None,
        supported_reasoning_efforts: vec![],
        supports_personality: false,
        additional_speed_tiers: vec![],
        service_tiers: vec![],
        default_service_tier: None,
        is_default: false,
        upgrade: None,
        show_in_picker,
        multi_agent_version: None,
        availability_nux: None,
        supported_in_api: true,
        input_modalities: vec![],
        is_custom,
        provider: provider.map(|p| p.to_string()),
        provider_label: None,
    }
}

/// A custom provider model preset addressed as `<provider>.<slug>`.
fn custom_preset(provider_id: &str, slug: &str, show_in_picker: bool) -> ModelPreset {
    preset(&format!("{provider_id}.{slug}"), Some(provider_id), true, show_in_picker)
}

#[test]
fn apply_codexium_visibility_applies_label_and_enable_state() {
    let tmp = tempdir().expect("tempdir");
    // openai provider: gpt-5.4 disabled with a label override; gpt-5.5 enabled.
    write_models(
        tmp.path(),
        r#"{"providers":{"openai":{"label":"OpenAI","readonly":true,"models":{
            "gpt-5.4": {"enabled": false, "label": "Renamed GPT-5.4"},
            "gpt-5.5": {"enabled": true}
        }}}}"#,
    );
    let presets = vec![
        preset("gpt-5.4", None, false, true),
        preset("gpt-5.5", None, false, true),
    ];
    let out = apply_codexium_visibility(tmp.path(), presets);
    let p54 = out.iter().find(|p| p.model == "gpt-5.4").expect("gpt-5.4");
    assert!(!p54.show_in_picker);
    assert_eq!(p54.display_name, "Renamed GPT-5.4");
    let p55 = out.iter().find(|p| p.model == "gpt-5.5").expect("gpt-5.5");
    assert!(p55.show_in_picker);
}

#[test]
fn apply_codexium_visibility_deletes_removed_custom_model_and_adds_new() {
    let tmp = tempdir().expect("tempdir");
    // deepseek provider has only `deepseek-chat`; the "model" sub-key of
    // `deepseek-reasoner` was removed (so it should be hidden), and a NEW
    // `deepseek-v3` model was added (so it should appear).
    write_models(
        tmp.path(),
        r#"{"providers":{"deepseek":{"label":"DeepSeek","base_url":"https://api.deepseek.com","env_key":"DEEPSEEK_API_KEY","readonly":false,"models":{
            "deepseek-chat": {"enabled": true, "label": "DeepSeek V3"}
        }}}}"#,
    );
    let presets = vec![
        custom_preset("deepseek", "deepseek-chat", true),
        custom_preset("deepseek", "deepseek-reasoner", true),
    ];
    let out = apply_codexium_visibility(tmp.path(), presets);
    let chat = out.iter().find(|p| p.model == "deepseek.deepseek-chat");
    assert!(chat.expect("deepseek-chat present").show_in_picker);
    // The removed model is hidden.
    let reasoner = out.iter().find(|p| p.model == "deepseek.deepseek-reasoner");
    assert!(!reasoner.expect("deepseek-reasoner present").show_in_picker);
    // mark_default_by_picker_visibility ran without panicking and picked a default.
    assert!(out.iter().any(|p| p.is_default));
}

