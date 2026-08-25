//! Codexium customization layer.
//!
//! Reads per-provider model metadata and API keys from a `codexium` folder
//! under the Codex home directory:
//!
//! - `<codex_home>/codexium/models.json` — per-provider, per-model overrides
//!   (`label`, `context_window`, `max_output_tokens`, ...).
//! - `<codex_home>/codexium/auth.json` — API keys keyed by provider id. Values
//!   are injected into the process environment under the provider's `env_key`
//!   so existing auth resolution picks them up.
//!
//! Both files are created automatically with sensible defaults when missing.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use tracing::warn;

use codex_model_provider_info::ModelProviderInfo;
use codex_model_provider_info::WireApi;
use codex_protocol::openai_models::ConfigShellToolType;
use codex_protocol::openai_models::ModelInfo;
use codex_protocol::openai_models::ModelPreset;
use codex_protocol::openai_models::ModelVisibility;
use codex_protocol::openai_models::ModelsResponse;
use codex_protocol::openai_models::TruncationPolicyConfig;

pub const CODEXIUM_DIR_NAME: &str = "codexium";
pub const MODELS_FILE_NAME: &str = "models.json";
pub const AUTH_FILE_NAME: &str = "auth.json";
pub const REGISTRY_FILE_NAME: &str = "providers-registry.json";

/// The bundled provider registry compiled into the binary. Serves as the
/// offline default; a newer same-major version fetched from the network
/// replaces it at runtime and is persisted to the codexium folder.
const BUNDLED_REGISTRY: &str = include_str!("providers_registry.json");

fn default_true() -> bool {
    true
}

/// Compare two dotted version strings like `1.0.0`. Returns `Greater` when `a`
/// is higher. Only the numeric segments are compared; non-numeric suffixes are
/// ignored.
fn cmp_version(a: &str, b: &str) -> std::cmp::Ordering {
    fn segs(v: &str) -> Vec<u64> {
        v.split('.')
            .filter_map(|s| {
                s.chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse()
                    .ok()
            })
            .collect()
    }
    let (sa, sb) = (segs(a), segs(b));
    for i in 0..sa.len().max(sb.len()) {
        let va = sa.get(i).copied().unwrap_or(0);
        let vb = sb.get(i).copied().unwrap_or(0);
        if va != vb {
            return va.cmp(&vb);
        }
    }
    std::cmp::Ordering::Equal
}

const DEFAULT_MODELS_JSON: &str = r#"{
  "providers": {}
}
"#;

const DEFAULT_AUTH_JSON: &str = r#"{
  "providers": {}
}
"#;

/// Parsed contents of `codexium/models.json`.
///
/// Shape:
/// ```json
/// { "providers": {
///     "<provider_id>": {
///       "label": "DeepSeek",
///       "models": { "<model_slug>": { ... } }
///     }
///   }
/// }
/// ```
/// A flat legacy form is also accepted where the provider value is the model map
/// directly: `{ "<provider_id>": { "<model_slug>": { ... } } }`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CodexiumModelsFile {
    #[serde(default)]
    pub providers: HashMap<String, CodexiumProviderConfig>,
}

/// Per-provider configuration stored under `providers.<provider>`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CodexiumProviderConfig {
    /// Friendly provider display name shown in the model picker.
    #[serde(default)]
    pub label: Option<String>,
    /// Base URL for the provider's OpenAI-compatible API. When set, the provider
    /// is registered in the model_providers map so requests route to this URL.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Environment variable that stores the user's API key for this provider.
    #[serde(default)]
    pub env_key: Option<String>,
    /// Which wire protocol this provider expects (`responses`).
    #[serde(default)]
    pub wire_api: Option<String>,
    /// Whether this provider requires a ChatGPT login. Custom providers set
    /// `false`.
    #[serde(default)]
    pub requires_openai_auth: bool,
    /// Read-only provider (e.g. OpenAI): the user can enable/disable its models
    /// but cannot edit or remove the provider itself.
    #[serde(default)]
    pub readonly: bool,
    /// `"openai"`, `"apiKey"`, or `"custom"`. Mirrors the registry catalog.
    #[serde(default)]
    pub provider_type: Option<String>,
    /// @lobehub/icons-static-svg icon id (e.g. `deepseek`).
    #[serde(default)]
    pub icon: Option<String>,
    /// Optional display name override (falls back to `label`).
    #[serde(default)]
    pub name: Option<String>,
    /// Per-model configuration keyed by model slug.
    #[serde(default)]
    pub models: HashMap<String, CodexiumModelConfig>,
}

impl<'de> Deserialize<'de> for CodexiumProviderConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Structured {
            #[serde(default)]
            label: Option<String>,
            #[serde(default)]
            base_url: Option<String>,
            #[serde(default)]
            env_key: Option<String>,
            #[serde(default)]
            wire_api: Option<String>,
            #[serde(default)]
            requires_openai_auth: bool,
            #[serde(default)]
            readonly: bool,
            #[serde(default)]
            provider_type: Option<String>,
            #[serde(default)]
            icon: Option<String>,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            models: HashMap<String, CodexiumModelConfig>,
        }
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.is_object() && value.get("models").is_some() {
            let structured: Structured =
                serde_json::from_value(value).map_err(serde::de::Error::custom)?;
            return Ok(CodexiumProviderConfig {
                label: structured.label,
                base_url: structured.base_url,
                env_key: structured.env_key,
                wire_api: structured.wire_api,
                requires_openai_auth: structured.requires_openai_auth,
                readonly: structured.readonly,
                provider_type: structured.provider_type,
                icon: structured.icon,
                name: structured.name,
                models: structured.models,
            });
        }
        // Legacy flat form: the whole object is the model map.
        let models = serde_json::from_value(value).map_err(serde::de::Error::custom)?;
        Ok(CodexiumProviderConfig {
            label: None,
            base_url: None,
            env_key: None,
            wire_api: None,
            requires_openai_auth: false,
            readonly: false,
            provider_type: None,
            icon: None,
            name: None,
            models,
        })
    }
}

/// Per-model configuration stored under `providers.<provider>.<model>`.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CodexiumModelConfig {
    /// Whether this model is enabled in the picker. Disabled models are kept
    /// in config but not offered.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Friendly display name.
    #[serde(default)]
    pub label: Option<String>,
    /// Short description shown in the model picker.
    #[serde(default)]
    pub description: Option<String>,
    /// Context window size in tokens.
    #[serde(default)]
    pub context_window: Option<i64>,
    /// Hard upper bound for the context window.
    #[serde(default)]
    pub max_context_window: Option<i64>,
    /// Token threshold that triggers auto-compaction.
    #[serde(default)]
    pub auto_compact_token_limit: Option<i64>,
    /// Maximum number of output tokens the model can produce in one turn.
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    /// Maximum number of tokens kept for a single tool output.
    #[serde(default)]
    pub tool_output_token_limit: Option<i64>,
    /// Shell execution capability for this model.
    #[serde(default)]
    pub shell_type: Option<ConfigShellToolType>,
}

/// Parsed contents of `codexium/auth.json`.
///
/// Accepts two shapes:
/// - Flat:  `{ "providers": { "<provider_id>": "<api_key>" } }`
/// - Nested: `{ "providers": { "<provider_id>": { "api_key": "<api_key>" } } }`
#[derive(Debug, Clone, Default, Serialize)]
pub struct CodexiumAuthFile {
    pub providers: HashMap<String, String>,
}

impl<'de> Deserialize<'de> for CodexiumAuthFile {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            providers: HashMap<String, serde_json::Value>,
        }
        let raw = Raw::deserialize(deserializer)?;
        let mut providers = HashMap::new();
        for (provider_id, value) in raw.providers {
            match value {
                serde_json::Value::String(api_key) => {
                    providers.insert(provider_id, api_key);
                }
                serde_json::Value::Object(map) => {
                    if let Some(serde_json::Value::String(api_key)) = map.get("api_key") {
                        providers.insert(provider_id, api_key.clone());
                    } else {
                        warn!(
                            "codexium auth provider `{provider_id}` has no `api_key` field; skipping"
                        );
                    }
                }
                _ => {
                    warn!(
                        "codexium auth provider `{provider_id}` has an invalid api key; skipping"
                    );
                }
            }
        }
        Ok(CodexiumAuthFile { providers })
    }
}

/// Returns the codexium folder for a given Codex home.
pub fn codexium_dir(codex_home: &Path) -> PathBuf {
    codex_home.join(CODEXIUM_DIR_NAME)
}

fn models_path(codex_home: &Path) -> PathBuf {
    codexium_dir(codex_home).join(MODELS_FILE_NAME)
}

fn auth_path(codex_home: &Path) -> PathBuf {
    codexium_dir(codex_home).join(AUTH_FILE_NAME)
}

fn write_default_if_missing(path: &Path, default: &str) -> std::io::Result<()> {
    if path.exists() {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, default)
}

/// Returns the bundled first-party OpenAI models as (slug, label, description)
/// tuples, used to build the read-only `openai` provider in the settings UI.
pub fn builtin_openai_models() -> Vec<(String, String, Option<String>)> {
    codex_models_manager::bundled_models_response()
        .map(|catalog| {
            catalog
                .models
                .into_iter()
                .filter(|model| model.slug != "codex-auto-review")
                .map(|model| {
                    (
                        model.slug.clone(),
                        model.display_name.clone(),
                        model.description.clone(),
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Ensures the codexium folder and both config files exist, creating them with
/// defaults when missing. Never overwrites user content.
pub fn ensure_default_files(codex_home: &Path) -> std::io::Result<()> {
    write_default_if_missing(&models_path(codex_home), DEFAULT_MODELS_JSON)?;
    write_default_if_missing(&auth_path(codex_home), DEFAULT_AUTH_JSON)?;
    Ok(())
}

/// Loads `codexium/models.json`. Returns an empty file model when the file is
/// absent or unparseable (logging a warning in the latter case).
pub fn load_models_file(codex_home: &Path) -> CodexiumModelsFile {
    let path = models_path(codex_home);
    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(file) => file,
            Err(err) => {
                warn!("failed to parse {}: {err}", path.display());
                CodexiumModelsFile::default()
            }
        },
        Err(err) => {
            warn!("failed to read {}: {err}", path.display());
            CodexiumModelsFile::default()
        }
    }
}

/// Loads `codexium/auth.json`. Returns an empty auth model when the file is
/// absent or unparseable.
pub fn load_auth_file(codex_home: &Path) -> CodexiumAuthFile {
    let path = auth_path(codex_home);
    match std::fs::read_to_string(&path) {
        Ok(contents) => match serde_json::from_str(&contents) {
            Ok(file) => file,
            Err(err) => {
                warn!("failed to parse {}: {err}", path.display());
                CodexiumAuthFile::default()
            }
        },
        Err(_) => CodexiumAuthFile::default(),
    }
}

/// Injects API keys from `auth.json` into the process environment.
///
/// For each provider id that has a key in `auth.json`, the value is written to
/// the environment variable named by the provider's `env_key` (when the
/// provider declares one). Real environment values are never overwritten, so a
/// key set in the shell wins over the file.
///
/// SAFETY: env mutation happens once during early config loading, before the
/// runtime spawns worker tasks. Concurrent access to the process environment is
/// not possible at this point.
pub fn apply_auth_to_env(
    auth: &CodexiumAuthFile,
    model_providers: &HashMap<String, ModelProviderInfo>,
) {
    for (provider_id, api_key) in &auth.providers {
        let Some(provider) = model_providers.get(provider_id) else {
            warn!("codexium auth has key for unknown provider `{provider_id}`, skipping");
            continue;
        };
        let Some(env_key) = provider.env_key.as_deref() else {
            warn!(
                "codexium auth provider `{provider_id}` has no `env_key` configured; key ignored"
            );
            continue;
        };
        if std::env::var_os(env_key).is_some() {
            continue;
        }
        // SAFETY: see the function-level safety comment.
        unsafe {
            std::env::set_var(env_key, api_key);
        }
    }
}

/// Builds a map of `ModelProviderInfo` from the providers declared in
/// `codexium/models.json`. Only providers that declare a `base_url` are
/// registered as runtime providers.
pub fn build_codexium_model_providers(codex_home: &Path) -> HashMap<String, ModelProviderInfo> {
    let file = load_models_file(codex_home);
    let mut providers = HashMap::new();
    for (provider_id, config) in &file.providers {
        let Some(base_url) = config.base_url.as_deref() else {
            continue;
        };
        let wire_api = match config.wire_api.as_deref() {
            Some("responses") | None => WireApi::Responses,
            Some(other) => {
                warn!(
                    "codexium provider `{provider_id}` has unsupported wire_api `{other}`; using responses"
                );
                WireApi::Responses
            }
        };
        let display_name = config
            .name
            .clone()
            .or_else(|| config.label.clone())
            .unwrap_or_else(|| provider_id.clone());
        let mut provider = ModelProviderInfo {
            name: display_name,
            base_url: Some(base_url.to_string()),
            env_key: config.env_key.clone(),
            wire_api,
            requires_openai_auth: config.requires_openai_auth,
            ..Default::default()
        };
        provider.requires_openai_auth = config.requires_openai_auth;
        providers.insert(provider_id.clone(), provider);
    }
    providers
}

/// Builds a model catalog combining the supplied catalog (or the bundled one)
/// with the per-provider models declared in `codexium/models.json`.
pub fn build_model_catalog(
    codex_home: &Path,
    existing_catalog: Option<ModelsResponse>,
) -> ModelsResponse {
    let file = load_models_file(codex_home);
    let mut models = existing_catalog
        .map(|catalog| catalog.models)
        .unwrap_or_else(|| {
            codex_models_manager::bundled_models_response()
                .map(|catalog| catalog.models)
                .unwrap_or_default()
        });

    for (provider_id, provider_config) in &file.providers {
        let provider_label = provider_config
            .label
            .clone()
            .unwrap_or_else(|| provider_id.clone());

        // The read-only `openai` provider references the bundled OpenAI models
        // by their *bare* slug, so we apply the codexium overrides (and the
        // enable/disable state) directly to those existing bundled models rather
        // than minting new `openai.<slug>` duplicates (which would split OpenAI
        // into two groups and leave disabled models visible).
        if provider_id == "openai" {
            for (model_slug, config) in &provider_config.models {
                apply_openai_model_override(&mut models, model_slug, config);
            }
            continue;
        }

        for (model_slug, config) in &provider_config.models {
            // Custom models are addressed as `<provider>.<model>` end to end.
            let full_slug = format!("{provider_id}.{model_slug}");
            let info = model_info_from_config(&full_slug, provider_id, &provider_label, config);
            if let Some(existing) = models.iter_mut().find(|m| m.slug == full_slug) {
                *existing = info;
            } else {
                models.push(info);
            }
        }
    }

    ModelsResponse { models }
}

/// Re-applies the freshly-read `codexium/models.json` state to the model
/// presets so provider/model changes (enable/disable, rename, add, delete, and
/// label/description edits) take effect without a relaunch. The startup-merged
/// catalog snapshot is otherwise frozen until the app restarts.
///
/// - Built-in OpenAI models are keyed by their bare slug (`gpt-5.4`).
/// - Custom provider models are keyed by `<provider>.<slug>` (e.g.
///   `deepseek.deepseek-chat`), which matches `preset.model`.
///
/// Concretely this:
/// 1. Re-applies enable/disable state and label/description overrides to every
///    existing preset that codexium manages.
/// 2. Hides presets for codexium custom models that were removed from config.
/// 3. Adds presets for newly-configured custom models that aren't present yet.
/// 4. Recomputes the default preset from the refreshed picker visibility.
pub fn apply_codexium_visibility(codex_home: &Path, presets: Vec<ModelPreset>) -> Vec<ModelPreset> {
    let file = load_models_file(codex_home);
    let mut present: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut out: Vec<ModelPreset> = Vec::with_capacity(presets.len() + 8);

    for mut preset in presets {
        match model_override(&file, &preset) {
            Some((enabled, label, description)) => {
                preset.show_in_picker = enabled;
                if let Some(label) = label {
                    preset.display_name = label;
                }
                if let Some(description) = description {
                    preset.description = description;
                }
                present.insert(preset.model.clone());
            }
            None if is_codexium_custom(&preset) => {
                // A codexium custom model that is no longer configured was
                // deleted/renamed — drop it from the picker.
                preset.show_in_picker = false;
            }
            None => {}
        }
        out.push(preset);
    }

    // Add any newly-configured custom provider models not already represented.
    for (provider_id, provider) in &file.providers {
        if provider_id == "openai" {
            continue;
        }
        let provider_label = provider
            .label
            .clone()
            .unwrap_or_else(|| provider_id.clone());
        for (model_slug, config) in &provider.models {
            if config.enabled && !present.contains(&format!("{provider_id}.{model_slug}")) {
                let preset = codexium_custom_model_preset(provider_id, &provider_label, model_slug, config);
                present.insert(preset.model.clone());
                out.push(preset);
            }
        }
    }

    ModelPreset::mark_default_by_picker_visibility(&mut out);
    out
}

/// Returns (enabled, label, description) for a preset's codexium config, or
/// `None` when the preset is not managed by codexium.
fn model_override(
    file: &CodexiumModelsFile,
    preset: &ModelPreset,
) -> Option<(bool, Option<String>, Option<String>)> {
    // Custom provider model: `<provider>.<slug>`, provider stored in `preset.provider`.
    if let Some(provider_id) = preset.provider.as_deref() {
        if provider_id != "openai" {
            let slug = preset
                .model
                .strip_prefix(&format!("{provider_id}."))
                .unwrap_or(&preset.model);
            return file
                .providers
                .get(provider_id)
                .and_then(|provider| provider.models.get(slug))
                .map(|m| (m.enabled, m.label.clone(), m.description.clone()));
        }
    }
    // Built-in OpenAI model: bare slug.
    file.providers
        .get("openai")
        .and_then(|provider| provider.models.get(&preset.model))
        .map(|m| (m.enabled, m.label.clone(), m.description.clone()))
}

/// True when a preset belongs to a codexium-managed custom provider (only those
/// presets carry a non-OpenAI `provider` id).
fn is_codexium_custom(preset: &ModelPreset) -> bool {
    matches!(preset.provider.as_deref(), Some(provider_id) if provider_id != "openai")
}

/// Builds a [`ModelPreset`] for a codexium custom model from its stored config.
fn codexium_custom_model_preset(
    provider_id: &str,
    provider_label: &str,
    model_slug: &str,
    config: &CodexiumModelConfig,
) -> ModelPreset {
    let full_slug = format!("{provider_id}.{model_slug}");
    let info = model_info_from_config(&full_slug, provider_id, provider_label, config);
    let mut preset: ModelPreset = info.into();
    preset.is_custom = true;
    preset.provider = Some(provider_id.to_string());
    preset.provider_label = Some(provider_label.to_string());
    preset
}

/// Applies a per-model codexium override to a bundled OpenAI model, keyed by its
/// bare slug. A disabled model is hidden from the picker; metadata overrides
/// (label/description/context/tokens) are merged in while keeping the model
/// grouped as part of the built-in OpenAI provider.
fn apply_openai_model_override(
    models: &mut Vec<ModelInfo>,
    model_slug: &str,
    config: &CodexiumModelConfig,
) {
    let Some(existing) = models.iter_mut().find(|m| m.slug == model_slug) else {
        return;
    };
    if let Some(label) = &config.label {
        existing.display_name = label.clone();
    }
    if let Some(description) = &config.description {
        existing.description = Some(description.clone());
    }
    if let Some(context_window) = config.context_window {
        existing.context_window = Some(context_window);
    }
    if let Some(max_context_window) = config.max_context_window {
        existing.max_context_window = Some(max_context_window);
    }
    if let Some(auto_compact_token_limit) = config.auto_compact_token_limit {
        existing.auto_compact_token_limit = Some(auto_compact_token_limit);
    }
    if let Some(max_output_tokens) = config.max_output_tokens {
        existing.max_output_tokens = Some(max_output_tokens);
    }
    if let Some(tool_output_token_limit) = config.tool_output_token_limit {
        existing.truncation_policy = TruncationPolicyConfig::tokens(tool_output_token_limit);
    }
    if let Some(shell_type) = config.shell_type {
        existing.shell_type = shell_type;
    }
    // Disabled OpenAI models no longer appear in the chat model picker.
    existing.visibility = if config.enabled {
        ModelVisibility::List
    } else {
        ModelVisibility::Hide
    };
    existing.used_fallback_model_metadata = false;
}

/// Builds a [`ModelInfo`] for a custom model, starting from the generic
/// fallback descriptor and applying the overrides from `codexium/models.json`.
fn model_info_from_config(
    slug: &str,
    provider_id: &str,
    provider_label: &str,
    config: &CodexiumModelConfig,
) -> ModelInfo {
    let mut info = codex_models_manager::model_info::model_info_from_slug(slug);
    if let Some(label) = &config.label {
        info.display_name = label.clone();
    }
    if let Some(description) = &config.description {
        info.description = Some(description.clone());
    }
    if let Some(context_window) = config.context_window {
        info.context_window = Some(context_window);
    }
    if let Some(max_context_window) = config.max_context_window {
        info.max_context_window = Some(max_context_window);
    }
    if let Some(auto_compact_token_limit) = config.auto_compact_token_limit {
        info.auto_compact_token_limit = Some(auto_compact_token_limit);
    }
    if let Some(max_output_tokens) = config.max_output_tokens {
        info.max_output_tokens = Some(max_output_tokens);
    }
    if let Some(tool_output_token_limit) = config.tool_output_token_limit {
        info.truncation_policy = TruncationPolicyConfig::tokens(tool_output_token_limit);
    }
    if let Some(shell_type) = config.shell_type {
        info.shell_type = shell_type;
    }
    info.visibility = ModelVisibility::List;
    info.priority = 10;
    info.used_fallback_model_metadata = false;
    info.is_custom = true;
    info.provider = Some(provider_id.to_string());
    info.provider_label = Some(provider_label.to_string());
    info
}

/// A single model entry in the provider registry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryModel {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub context_window: Option<i64>,
    #[serde(default)]
    pub max_output_tokens: Option<i64>,
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub description: Option<String>,
}

/// A single provider definition in the registry.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryProvider {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    /// `"openai"` for ChatGPT-login providers, `"apiKey"` for providers that
    /// only need an API key, `"custom"` otherwise.
    #[serde(default, rename = "type")]
    pub provider_type: String,
    #[serde(default)]
    pub recommended: bool,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub env_key: Option<String>,
    /// @lobehub/icons-static-svg icon id (e.g. `deepseek`).
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub models: Vec<RegistryModel>,
}

/// The provider registry document. Contains a version for upgrade detection.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRegistry {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub providers: Vec<RegistryProvider>,
}

/// Path to the persisted (network-refreshed) registry under the codexium dir.
fn registry_path(codex_home: &Path) -> PathBuf {
    codexium_dir(codex_home).join(REGISTRY_FILE_NAME)
}

/// Returns the compiled-in default registry, parsed. Falls back to an empty
/// registry on parse failure.
pub fn bundled_registry() -> ProviderRegistry {
    serde_json::from_str(BUNDLED_REGISTRY).unwrap_or_default()
}

/// Loads the provider registry. The persisted copy in the codexium folder is
/// used when present and newer-or-equal in version to the bundled default;
/// otherwise the bundled default is used.
pub fn load_registry(codex_home: &Path) -> ProviderRegistry {
    let bundled = bundled_registry();
    if let Ok(contents) = std::fs::read_to_string(registry_path(codex_home)) {
        if let Ok(remote) = serde_json::from_str::<ProviderRegistry>(&contents) {
            let use_remote =
                cmp_version(&remote.version, &bundled.version) != std::cmp::Ordering::Less;
            if use_remote {
                return remote;
            }
        }
    }
    bundled
}

/// Persists a registry to the codexium folder. Used to store a refreshed copy
/// fetched from the network.
pub fn save_registry(codex_home: &Path, registry: &ProviderRegistry) -> std::io::Result<()> {
    let path = registry_path(codex_home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(
        path,
        serde_json::to_string_pretty(registry).unwrap_or_default(),
    )
}

/// Compares a freshly fetched registry to the currently effective one. Returns
/// `true` when the fetched registry has the same major version but a higher
/// sub-version, meaning it should replace the local copy.
pub fn is_registry_update(fetched: &ProviderRegistry, current: &ProviderRegistry) -> bool {
    let major = |v: &str| v.split('.').next().unwrap_or("0").to_string();
    if major(&fetched.version) != major(&current.version) {
        return false;
    }
    cmp_version(&fetched.version, &current.version) == std::cmp::Ordering::Greater
}

#[cfg(test)]
#[path = "codexium_tests.rs"]
mod tests;
