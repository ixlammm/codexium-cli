//! Codexium: custom provider/model config management (Codexium Patch).
//!
//! Exposes `codexium/models/read` and `codexium/models/write` so the app can
//! edit `codexium/models.json` from the settings UI.

use std::collections::HashMap;
use std::path::PathBuf;

use codex_app_server_protocol::CodexiumModelSettings;
use codex_app_server_protocol::CodexiumModelsReadParams;
use codex_app_server_protocol::CodexiumModelsReadResponse;
use codex_app_server_protocol::CodexiumModelsWriteParams;
use codex_app_server_protocol::CodexiumModelsWriteResponse;
use codex_app_server_protocol::CodexiumProviderSettings;
use codex_app_server_protocol::CodexiumProviderStatus;
use codex_app_server_protocol::CodexiumProvidersCheckParams;
use codex_app_server_protocol::CodexiumProvidersCheckResponse;
use codex_app_server_protocol::CodexiumProvidersConnectParams;
use codex_app_server_protocol::CodexiumProvidersConnectResponse;
use codex_app_server_protocol::CodexiumProvidersDisconnectParams;
use codex_app_server_protocol::CodexiumProvidersDisconnectResponse;
use codex_app_server_protocol::CodexiumRegistryModel;
use codex_app_server_protocol::CodexiumRegistryProvider;
use codex_app_server_protocol::CodexiumRegistryReadResponse;
use codex_app_server_protocol::JSONRPCErrorError;

use codex_core::codexium::ProviderRegistry as CoreProviderRegistry;
use codex_core::codexium::RegistryModel as CoreRegistryModel;
use codex_core::codexium::RegistryProvider as CoreRegistryProvider;

use crate::error_code::internal_error;

#[derive(Clone)]
pub(crate) struct CodexiumRequestProcessor {
    codex_home: PathBuf,
}

impl CodexiumRequestProcessor {
    pub(crate) fn new(codex_home: PathBuf) -> Self {
        Self { codex_home }
    }

    fn models_path(&self) -> PathBuf {
        codex_core::codexium::codexium_dir(&self.codex_home)
            .join(codex_core::codexium::MODELS_FILE_NAME)
    }

    /// Returns a synthetic read-only `openai` provider listing the bundled
    /// OpenAI models, so the user can enable/disable them without removing.
    fn openai_provider(&self) -> CodexiumProviderSettings {
        let bundled = codex_core::codexium::builtin_openai_models();
        let models = bundled
            .into_iter()
            .map(|(slug, label, description)| {
                (
                    slug,
                    CodexiumModelSettings {
                        enabled: true,
                        label: Some(label),
                        description,
                        context_window: None,
                        max_context_window: None,
                        auto_compact_token_limit: None,
                        max_output_tokens: None,
                        tool_output_token_limit: None,
                    },
                )
            })
            .collect();
        CodexiumProviderSettings {
            label: Some("OpenAI".to_string()),
            base_url: None,
            env_key: None,
            wire_api: Some("responses".to_string()),
            requires_openai_auth: true,
            readonly: true,
            provider_type: Some("openai".to_string()),
            icon: Some("openai".to_string()),
            models,
        }
    }

    pub(crate) async fn models_read(
        &self,
        _params: CodexiumModelsReadParams,
    ) -> Result<CodexiumModelsReadResponse, JSONRPCErrorError> {
        codex_core::codexium::ensure_default_files(&self.codex_home)
            .map_err(|err| internal_error(format!("codexium init: {err}")))?;
        let file = codex_core::codexium::load_models_file(&self.codex_home);
        let stored = file.providers;
        let mut providers: HashMap<String, CodexiumProviderSettings> = stored
            .into_iter()
            .map(|(provider_id, config)| {
                let models = config
                    .models
                    .into_iter()
                    .map(|(slug, model)| {
                        (
                            slug,
                            CodexiumModelSettings {
                                enabled: model.enabled,
                                label: model.label,
                                description: model.description,
                                context_window: model.context_window,
                                max_context_window: model.max_context_window,
                                auto_compact_token_limit: model.auto_compact_token_limit,
                                max_output_tokens: model.max_output_tokens,
                                tool_output_token_limit: model.tool_output_token_limit,
                            },
                        )
                    })
                    .collect();
                (
                    provider_id,
                    CodexiumProviderSettings {
                        label: config.label,
                        base_url: config.base_url,
                        env_key: config.env_key,
                        wire_api: config.wire_api,
                        requires_openai_auth: config.requires_openai_auth,
                        readonly: config.readonly,
                        provider_type: config.provider_type,
                        icon: config.icon,
                        models,
                    },
                )
            })
            .collect();

        // Always expose the read-only OpenAI provider. Overlay any stored
        // enable/disable state captured for the `openai` provider.
        let openai_provider = self.openai_provider();
        let openai_state = providers.remove("openai");
        if let Some(state) = openai_state {
            let mut merged = openai_provider;
            for (slug, model) in state.models {
                if let Some(existing) = merged.models.get_mut(&slug) {
                    existing.enabled = model.enabled;
                } else {
                    merged.models.insert(slug, model);
                }
            }
            providers.insert("openai".to_string(), merged);
        } else {
            providers.insert("openai".to_string(), openai_provider);
        }

        Ok(CodexiumModelsReadResponse { providers })
    }

    pub(crate) async fn models_write(
        &self,
        params: CodexiumModelsWriteParams,
    ) -> Result<CodexiumModelsWriteResponse, JSONRPCErrorError> {
        codex_core::codexium::ensure_default_files(&self.codex_home)
            .map_err(|err| internal_error(format!("codexium init: {err}")))?;
        let providers = params
            .providers
            .into_iter()
            .filter(|(provider_id, provider)| {
                // Never persist the synthetic read-only OpenAI provider as a
                // full provider; only its per-model enable state is stored.
                provider_id != "openai" || provider.readonly
            })
            .map(|(provider_id, provider)| {
                let models = provider
                    .models
                    .into_iter()
                    .map(|(slug, model)| {
                        (
                            slug,
                            codex_core::codexium::CodexiumModelConfig {
                                enabled: model.enabled,
                                label: model.label,
                                description: model.description,
                                context_window: model.context_window,
                                max_context_window: model.max_context_window,
                                auto_compact_token_limit: model.auto_compact_token_limit,
                                max_output_tokens: model.max_output_tokens,
                                tool_output_token_limit: model.tool_output_token_limit,
                                shell_type: None,
                            },
                        )
                    })
                    .collect();
                (
                    provider_id,
                    codex_core::codexium::CodexiumProviderConfig {
                        label: provider.label,
                        base_url: provider.base_url,
                        env_key: provider.env_key,
                        wire_api: provider.wire_api,
                        requires_openai_auth: provider.requires_openai_auth,
                        readonly: provider.readonly,
                        name: None,
                        provider_type: provider.provider_type,
                        icon: provider.icon,
                        models,
                    },
                )
            })
            .collect();
        let file = codex_core::codexium::CodexiumModelsFile { providers };
        let contents = serde_json::to_string_pretty(&file)
            .map_err(|err| internal_error(format!("serialize codexium models: {err}")))?;
        std::fs::write(self.models_path(), contents)
            .map_err(|err| internal_error(format!("write codexium models: {err}")))?;
        Ok(CodexiumModelsWriteResponse {})
    }

    fn auth_path(&self) -> PathBuf {
        codex_core::codexium::codexium_dir(&self.codex_home)
            .join(codex_core::codexium::AUTH_FILE_NAME)
    }

    /// Reads the raw `providers` map from `auth.json`.
    fn read_auth_providers(&self) -> HashMap<String, String> {
        codex_core::codexium::load_auth_file(&self.codex_home).providers
    }

    /// Writes the full `providers` map back to `auth.json`.
    fn write_auth_providers(
        &self,
        providers: &HashMap<String, String>,
    ) -> Result<(), JSONRPCErrorError> {
        let file = codex_core::codexium::CodexiumAuthFile {
            providers: providers.clone(),
        };
        let contents = serde_json::to_string_pretty(&file)
            .map_err(|err| internal_error(format!("serialize codexium auth: {err}")))?;
        std::fs::write(self.auth_path(), contents)
            .map_err(|err| internal_error(format!("write codexium auth: {err}")))?;
        Ok(())
    }

    /// The network URL for the provider registry. A placeholder for now, so the
    /// fetch degrades gracefully to the bundled registry on any failure.
    const REGISTRY_URL: &'static str = "https://raw.githubusercontent.com/codexium/providers-registry/main/providers-registry.json";

    pub(crate) async fn registry_read(
        &self,
    ) -> Result<CodexiumRegistryReadResponse, JSONRPCErrorError> {
        codex_core::codexium::ensure_default_files(&self.codex_home)
            .map_err(|err| internal_error(format!("codexium init: {err}")))?;
        let current = codex_core::codexium::load_registry(&self.codex_home);
        let mut refreshed = false;

        // Try a network refresh: fetch the raw GitHub copy and, when it is the
        // same major version but a higher sub-version, persist and use it.
        if let Some(fetched) = self.fetch_registry_async().await {
            if codex_core::codexium::is_registry_update(&fetched, &current) {
                if let Err(err) = codex_core::codexium::save_registry(&self.codex_home, &fetched) {
                    tracing::warn!("codexium: failed to persist refreshed registry: {err}");
                }
                refreshed = true;
                return Ok(self.registry_response(fetched, refreshed));
            }
        }

        Ok(self.registry_response(current, refreshed))
    }

    async fn fetch_registry_async(&self) -> Option<CoreProviderRegistry> {
        // Direct (no outbound proxy) HTTP GET, matching the offline-tolerant
        // registry fetch semantics. Any failure degrades to the bundled copy.
        let client = codex_http_client::HttpClientBuilder::new()
            .connect_timeout(std::time::Duration::from_secs(6))
            .build_direct()
            .ok()?;
        let resp = client.get(Self::REGISTRY_URL).send().await.ok()?;
        if !resp.status().is_success() {
            return None;
        }
        serde_json::from_str(&resp.text().await.ok()?).ok()
    }

    fn registry_response(
        &self,
        registry: CoreProviderRegistry,
        refreshed: bool,
    ) -> CodexiumRegistryReadResponse {
        let providers = registry
            .providers
            .into_iter()
            .map(registry_provider_to_wire)
            .collect();
        CodexiumRegistryReadResponse {
            version: registry.version,
            providers,
            refreshed,
        }
    }

    pub(crate) async fn providers_connect(
        &self,
        params: CodexiumProvidersConnectParams,
    ) -> Result<CodexiumProvidersConnectResponse, JSONRPCErrorError> {
        codex_core::codexium::ensure_default_files(&self.codex_home)
            .map_err(|err| internal_error(format!("codexium init: {err}")))?;

        // Never connect over the reserved OpenAI provider id.
        if params.provider_id == "openai" {
            return Err(internal_error(
                "cannot connect the reserved `openai` provider",
            ));
        }

        let mut file = codex_core::codexium::load_models_file(&self.codex_home);
        let mut provider = file
            .providers
            .get(&params.provider_id)
            .cloned()
            .unwrap_or_default();

        if let Some(label) = params.label {
            provider.label = Some(label);
        }
        if let Some(base_url) = params.base_url {
            provider.base_url = Some(base_url);
        }
        if let Some(env_key) = params.env_key {
            provider.env_key = Some(env_key);
        }
        if let Some(wire_api) = params.provider_type.as_deref() {
            let _ = wire_api;
        }
        if let Some(provider_type) = params.provider_type {
            provider.provider_type = Some(provider_type);
        }
        if let Some(icon) = params.icon {
            provider.icon = Some(icon);
        }
        provider.requires_openai_auth = false;
        provider.readonly = false;
        provider.wire_api = Some("responses".to_string());
        if provider.env_key.is_none() {
            provider.env_key = Some(format!("{}_API_KEY", params.provider_id.to_uppercase()));
        }

        // Seed models from the registry (if any provided), preserving any
        // existing per-model enable state already stored for this provider.
        let existing = provider.models.clone();
        let mut models = HashMap::new();
        for model in params.models {
            let slug = model.name.clone();
            if let Some(prev) = existing.get(&slug) {
                models.insert(slug.clone(), prev.clone());
                continue;
            }
            models.insert(
                slug.clone(),
                codex_core::codexium::CodexiumModelConfig {
                    enabled: true,
                    label: model.label.clone(),
                    description: model.description.clone(),
                    context_window: model.context_window,
                    max_context_window: None,
                    auto_compact_token_limit: None,
                    max_output_tokens: model.max_output_tokens,
                    tool_output_token_limit: None,
                    shell_type: None,
                },
            );
        }

        if !models.is_empty() {
            provider.models = models;
        }
        file.providers.insert(params.provider_id.clone(), provider);

        let contents = serde_json::to_string_pretty(&file)
            .map_err(|err| internal_error(format!("serialize codexium models: {err}")))?;
        std::fs::write(self.models_path(), contents)
            .map_err(|err| internal_error(format!("write codexium models: {err}")))?;

        // Store the API key (if any) into auth.json and inject it into the
        // environment under the provider's env_key.
        if let Some(api_key) = params.api_key {
            if !api_key.is_empty() {
                let mut auth = self.read_auth_providers();
                auth.insert(params.provider_id.clone(), api_key.clone());
                self.write_auth_providers(&auth)?;
                let env_key = file.providers[&params.provider_id].env_key.clone();
                if let Some(env_key) = env_key {
                    if std::env::var_os(&env_key).is_none() {
                        // SAFETY: best-effort, matches apply_auth_to_env semantics;
                        // called once during a config update before worker tasks.
                        unsafe {
                            std::env::set_var(&env_key, &api_key);
                        }
                    }
                }
            }
        }

        Ok(CodexiumProvidersConnectResponse {})
    }

    pub(crate) async fn providers_disconnect(
        &self,
        params: CodexiumProvidersDisconnectParams,
    ) -> Result<CodexiumProvidersDisconnectResponse, JSONRPCErrorError> {
        codex_core::codexium::ensure_default_files(&self.codex_home)
            .map_err(|err| internal_error(format!("codexium init: {err}")))?;

        if params.provider_id == "openai" {
            return Err(internal_error(
                "cannot disconnect the reserved `openai` provider",
            ));
        }

        let mut file = codex_core::codexium::load_models_file(&self.codex_home);
        file.providers.remove(&params.provider_id);
        let contents = serde_json::to_string_pretty(&file)
            .map_err(|err| internal_error(format!("serialize codexium models: {err}")))?;
        std::fs::write(self.models_path(), contents)
            .map_err(|err| internal_error(format!("write codexium models: {err}")))?;

        let mut auth = self.read_auth_providers();
        auth.remove(&params.provider_id);
        self.write_auth_providers(&auth)?;

        Ok(CodexiumProvidersDisconnectResponse {})
    }

    /// Checks a provider's connectivity by issuing a lightweight request to its
    /// base URL (or its `/models` endpoint) and timing the round trip. Returns
    /// one status per provider (or just the requested one).
    pub(crate) async fn providers_check(
        &self,
        params: CodexiumProvidersCheckParams,
    ) -> Result<CodexiumProvidersCheckResponse, JSONRPCErrorError> {
        let file = codex_core::codexium::load_models_file(&self.codex_home);
        let auth = codex_core::codexium::load_auth_file(&self.codex_home);

        let ids: Vec<String> = match &params.provider_id {
            Some(id) => vec![id.clone()],
            None => file
                .providers
                .keys()
                .cloned()
                .filter(|id| id != "openai")
                .collect(),
        };

        let client = codex_http_client::HttpClientBuilder::new()
            .connect_timeout(std::time::Duration::from_secs(8))
            .build_direct()
            .map_err(|err| internal_error(format!("codexium http client: {err}")))?;

        let mut statuses = Vec::new();
        for provider_id in ids {
            let Some(provider) = file.providers.get(&provider_id) else {
                statuses.push(CodexiumProviderStatus {
                    provider_id: provider_id.clone(),
                    ok: false,
                    message: "provider not configured".to_string(),
                    latency_ms: 0,
                    checked_at: chrono::Utc::now().timestamp(),
                });
                continue;
            };
            let base_url = provider.base_url.clone().unwrap_or_default();
            let env_key = provider.env_key.clone();
            let started = std::time::Instant::now();
            let (ok, message) = self
                .check_provider(&client, &provider_id, &base_url, env_key.as_deref(), &auth)
                .await;
            statuses.push(CodexiumProviderStatus {
                provider_id: provider_id.clone(),
                ok,
                message,
                latency_ms: started.elapsed().as_millis() as i64,
                checked_at: chrono::Utc::now().timestamp(),
            });
        }

        Ok(CodexiumProvidersCheckResponse { statuses })
    }

    /// Issues a GET to `<base>/models` (OpenAI-compatible) or the base URL and
    /// reports whether the API key is accepted.
    async fn check_provider(
        &self,
        client: &codex_http_client::HttpClient,
        provider_id: &str,
        base_url: &str,
        env_key: Option<&str>,
        auth: &codex_core::codexium::CodexiumAuthFile,
    ) -> (bool, String) {
        if base_url.is_empty() {
            return (false, "no base URL configured".to_string());
        }
        // Normalize: strip trailing slash, append /models (a cheap, key-checking
        // endpoint for most OpenAI-compatible providers).
        let base = base_url.trim_end_matches('/');
        let url = format!("{base}/models");
        let api_key = auth.providers.get(provider_id);
        let mut builder = client.get(&url);
        if let Some(key) = api_key {
            builder = builder.header("Authorization", format!("Bearer {key}"));
        } else if let Some(env_key) = env_key {
            if let Ok(k) = std::env::var(env_key) {
                builder = builder.header("Authorization", format!("Bearer {k}"));
            }
        }
        match builder.send().await {
            Ok(resp) => {
                let status = resp.status();
                match status.as_u16() {
                    200 => (true, format!("OK ({status})")),
                    401 | 403 => (false, format!("unauthorized ({status})")),
                    404 => (false, format!("not found ({status})")),
                    other => (true, format!("responded ({other})")),
                }
            }
            Err(err) => (false, err.to_string()),
        }
    }
}

fn registry_model_to_wire(model: CoreRegistryModel) -> CodexiumRegistryModel {
    CodexiumRegistryModel {
        name: model.name,
        label: model.label,
        context_window: model.context_window,
        max_output_tokens: model.max_output_tokens,
        input: model.input,
        output: model.output,
        description: model.description,
    }
}

fn registry_provider_to_wire(provider: CoreRegistryProvider) -> CodexiumRegistryProvider {
    CodexiumRegistryProvider {
        id: provider.id,
        name: provider.name,
        provider_type: provider.provider_type,
        recommended: provider.recommended,
        description: provider.description,
        base_url: provider.base_url,
        env_key: provider.env_key,
        icon: provider.icon,
        models: provider
            .models
            .into_iter()
            .map(registry_model_to_wire)
            .collect(),
    }
}
