use super::*;

use codex_connectors::ConnectorRuntimeTool;
use codex_connectors::connector_runtime_context_key;
use codex_connectors::connector_tool_is_synthetic;
use codex_connectors::installed_connector_runtime;
use codex_mcp::CODEX_APPS_MCP_SERVER_NAME;
use codex_mcp::MCP_TOOL_CODEX_APPS_META_KEY;
use codex_mcp::McpRuntime;
use codex_mcp::McpRuntimeInput;
use codex_mcp::McpStartupPolicy;
use codex_mcp::ToolInfo;
use codex_mcp::effective_mcp_servers;
use codex_mcp::host_owned_codex_apps_enabled;
use codex_mcp::tool_is_model_visible;
use codex_protocol::mcp::ClientMcpExtensions;
use codex_protocol::models::PermissionProfile;

const CONNECTOR_RUNTIME_REFRESH_TIMEOUT: Duration = Duration::from_secs(30);
const APPS_INSTALLED_SUBMIT_ID: &str = "app-installed";

impl AppsRequestProcessor {
    pub(crate) async fn apps_installed(
        &self,
        params: AppsInstalledParams,
    ) -> Result<AppsInstalledResponse, JSONRPCErrorError> {
        let force_refresh = params.force_refresh;
        let result = async {
            let config = self
                .load_apps_config(params.thread_id.as_deref())
                .await?;
            let auth = self.auth_manager.auth().await;
            let apps_enabled = config
                .features
                .apps_enabled_for_auth(auth.as_ref().is_some_and(CodexAuth::uses_codex_backend));

            let workspace_enabled = apps_enabled
                && self
                    .workspace_codex_plugins_enabled(&config, auth.as_ref())
                    .await;
            let runtime_enabled = apps_enabled && workspace_enabled;

            let mcp_manager = self.thread_manager.mcp_manager();
            let mut mcp_config = mcp_manager.runtime_config(&config).await;
            // Installed-app discovery has no active turn or reviewer.
            mcp_config.permission_profile = PermissionProfile::default();
            let mcp_config = Arc::new(mcp_config);
            let mut mcp_servers = effective_mcp_servers(&mcp_config, auth.as_ref());
            mcp_servers.retain(|name, _| name == CODEX_APPS_MCP_SERVER_NAME);
            let cache_key = connector_runtime_context_key(auth.as_ref());
            let previous_snapshot = mcp_manager
                .codex_apps_tools_cache()
                .current_snapshot(config.codex_home.to_path_buf(), cache_key.clone());
            let snapshot = if force_refresh && runtime_enabled {
                let refresh_result = async {
                    anyhow::ensure!(
                        !mcp_servers.is_empty(),
                        "host-owned MCP server '{CODEX_APPS_MCP_SERVER_NAME}' is not enabled"
                    );
                    let startup_timeout = mcp_servers
                        .get(CODEX_APPS_MCP_SERVER_NAME)
                        .and_then(|server| server.config().startup_timeout_sec)
                        .unwrap_or(CONNECTOR_RUNTIME_REFRESH_TIMEOUT);
                    let runtime_context = McpRuntimeContext::new(
                        self.thread_manager.environment_manager(),
                        config.cwd.to_path_buf(),
                    );
                    let cancellation_token = CancellationToken::new();
                    let codex_apps_auth_manager =
                        host_owned_codex_apps_enabled(&mcp_config, auth.as_ref())
                            .then(|| Arc::clone(&self.auth_manager));
                    let runtime = McpRuntime::new(McpRuntimeInput {
                        startup_policy: McpStartupPolicy::Eager,
                        config: Arc::clone(&mcp_config),
                        plugins_available: false,
                        ready_selected_capability_roots: Vec::new(),
                        mcp_servers,
                        submit_id: APPS_INSTALLED_SUBMIT_ID.to_string(),
                        tx_event: None,
                        startup_cancellation_token: cancellation_token.clone(),
                        runtime_context,
                        codex_apps_tools_cache: mcp_manager.codex_apps_tools_cache(),
                        tool_catalog_cache: mcp_manager.tool_catalog_cache(),
                        codex_apps_tools_cache_key: cache_key.clone(),
                        client_mcp_extensions: ClientMcpExtensions::default(),
                        auth: auth.clone(),
                        codex_apps_auth_manager,
                        elicitation_reviewer: None,
                        elicitation_lifecycle: None,
                    })
                    .await;

                    let result = if runtime
                        .latest_wait_for_server_ready(
                            CODEX_APPS_MCP_SERVER_NAME,
                            startup_timeout,
                        )
                        .await
                    {
                        mcp_manager
                            .codex_apps_tools_cache()
                            .current_snapshot(config.codex_home.to_path_buf(), cache_key.clone())
                            .ok_or_else(|| {
                                anyhow::anyhow!(
                                    "hosted connector refresh completed without publishing a snapshot"
                                )
                            })
                    } else {
                        Err(anyhow::anyhow!(
                            "failed to refresh tools for MCP server '{CODEX_APPS_MCP_SERVER_NAME}'"
                        ))
                    };
                    cancellation_token.cancel();
                    runtime.shutdown().await;
                    result
                }
                .await;

                match refresh_result {
                    Ok(snapshot) => Some(snapshot),
                    Err(err) => {
                        return Err(internal_error(format!(
                            "failed to refresh installed connector runtime state: {err:#}"
                        )));
                    }
                }
            } else {
                previous_snapshot
            };
            let Some(snapshot) = snapshot else {
                return Ok(AppsInstalledResponse { apps: Vec::new() });
            };

            let apps = installed_connector_runtime(
                &config.config_layer_stack,
                snapshot.tools().iter().map(connector_runtime_tool),
            )
            .into_iter()
            .map(|app| InstalledApp {
                id: app.id,
                runtime_name: app.runtime_name,
                enabled: runtime_enabled && app.enabled,
                callable: runtime_enabled && app.callable,
            })
            .collect();
            Ok(AppsInstalledResponse { apps })
        }
        .await;

        result
    }
}

fn connector_runtime_tool(tool: &ToolInfo) -> ConnectorRuntimeTool<'_> {
    let annotations = tool.tool.annotations.as_ref();
    ConnectorRuntimeTool {
        connector_id: tool.connector_id.as_deref(),
        connector_name: tool.connector_name.as_deref(),
        tool_name: &tool.tool.name,
        tool_title: tool.tool.title.as_deref(),
        destructive_hint: annotations.and_then(|annotations| annotations.destructive_hint),
        open_world_hint: annotations.and_then(|annotations| annotations.open_world_hint),
        synthetic: connector_tool_is_synthetic(
            tool.tool
                .meta
                .as_deref()
                .and_then(|meta| meta.get(MCP_TOOL_CODEX_APPS_META_KEY)),
        ),
        model_visible: tool_is_model_visible(tool),
    }
}
