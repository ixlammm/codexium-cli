//! Codexium: per-provider custom model management (Codexium Patch).
//!
//! These RPCs let the app read and edit `codexium/models.json`, the user's
//! custom provider/model configuration.

use crate::JsonSchema;
use crate::TS;
use serde::Deserialize;
use serde::Serialize;

fn default_true() -> bool {
    true
}

/// Params for `codexium/models/read`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumModelsReadParams {}

/// A single custom model as stored in `codexium/models.json`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumModelSettings {
    /// Whether this model is enabled in the picker. Kept (not removed) when
    /// disabled.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Friendly display name for the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Short description shown in the model picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Context window size in tokens.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
    /// Hard upper bound for the context window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_context_window: Option<i64>,
    /// Token threshold that triggers auto-compaction.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_compact_token_limit: Option<i64>,
    /// Maximum number of output tokens the model can produce in one turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i64>,
    /// Maximum number of tokens kept for a single tool output.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_output_token_limit: Option<i64>,
}

/// A custom provider as stored in `codexium/models.json`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumProviderSettings {
    /// Friendly provider display name shown in the model picker.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    /// Base URL for the provider's OpenAI-compatible API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Environment variable that stores the user's API key for this provider.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    /// Which wire protocol this provider expects (`responses`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wire_api: Option<String>,
    /// Whether this provider requires a ChatGPT login. Custom providers set
    /// `false`.
    #[serde(default)]
    pub requires_openai_auth: bool,
    /// Read-only provider (e.g. OpenAI): models can be toggled but the provider
    /// cannot be edited or removed.
    #[serde(default)]
    pub readonly: bool,
    /// `"openai"`, `"apiKey"`, or `"custom"`. Mirrors the registry catalog.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    /// @lobehub/icons-static-svg icon id (e.g. `deepseek`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    /// Per-model configuration keyed by model slug.
    #[serde(default)]
    pub models: std::collections::HashMap<String, CodexiumModelSettings>,
}

// ---------------------------------------------------------------------------
// Provider registry (the bundled / network-refreshed catalog of predefined
// providers). Read-only reference; connecting copies a provider into models.json.
// ---------------------------------------------------------------------------

/// A single model entry in the provider registry.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumRegistryModel {
    #[serde(default)]
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

/// A single provider definition in the provider registry.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumRegistryProvider {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub provider_type: String,
    #[serde(default)]
    pub recommended: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default)]
    pub models: Vec<CodexiumRegistryModel>,
}

/// Params for `codexium/registry/read`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumRegistryReadParams {}

/// Response for `codexium/registry/read`. `refreshed` indicates whether the
/// returned registry came from the network refresh (vs the bundled default).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumRegistryReadResponse {
    pub version: String,
    pub providers: Vec<CodexiumRegistryProvider>,
    pub refreshed: bool,
}

// ---------------------------------------------------------------------------
// Providers page: connect / disconnect a provider and write its API key.
// ---------------------------------------------------------------------------

/// Params for `codexium/providers/connect`. Copies a (registry or custom)
/// provider into `models.json` and optionally stores its API key.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumProvidersConnectParams {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(default)]
    pub models: Vec<CodexiumRegistryModel>,
}

/// Response for `codexium/providers/connect`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumProvidersConnectResponse {}

/// Params for `codexium/providers/disconnect`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumProvidersDisconnectParams {
    pub provider_id: String,
}

/// Response for `codexium/providers/disconnect`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumProvidersDisconnectResponse {}

/// The health of a single provider, checked by hitting its API.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumProviderStatus {
    pub provider_id: String,
    /// `true` when the API responded successfully to a lightweight request.
    pub ok: bool,
    /// Human-readable detail (error message or "OK").
    pub message: String,
    /// Duration of the check in milliseconds.
    pub latency_ms: i64,
    /// Unix seconds when the check ran.
    pub checked_at: i64,
}

/// Params for `codexium/providers/check`. Omit `providerId` to check all.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumProvidersCheckParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_id: Option<String>,
}

/// Response for `codexium/providers/check`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumProvidersCheckResponse {
    pub statuses: Vec<CodexiumProviderStatus>,
}

/// Response for `codexium/models/read`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumModelsReadResponse {
    /// Custom providers keyed by provider id.
    pub providers: std::collections::HashMap<String, CodexiumProviderSettings>,
}

/// Params for `codexium/models/write`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumModelsWriteParams {
    /// The full set of custom providers to persist.
    pub providers: std::collections::HashMap<String, CodexiumProviderSettings>,
}

/// Response for `codexium/models/write`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, JsonSchema, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "v2/")]
pub struct CodexiumModelsWriteResponse {}
