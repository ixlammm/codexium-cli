use std::path::Path;

use crate::model::ExternalAgentConfigImportItemResult;
use crate::model::ExternalAgentConfigImportRawError;
use crate::model::PluginImportOutcome;

pub fn record_import_error(
    result: &mut ExternalAgentConfigImportItemResult,
    failure_stage: &'static str,
    sub_error_type: Option<&str>,
    message: impl Into<String>,
    source: Option<String>,
) {
    result.record_error(ExternalAgentConfigImportRawError {
        item_type: result.item_type,
        error_type: None,
        sub_error_type: sub_error_type.map(str::to_string),
        failure_stage: failure_stage.to_string(),
        message: message.into(),
        cwd: result.cwd.clone(),
        source,
    });
}

pub(super) fn record_plugin_import_errors(
    outcome: &mut PluginImportOutcome,
    cwd: Option<&Path>,
    plugin_ids: &[String],
    failure_stage: &'static str,
    message: impl Into<String>,
) {
    let message = message.into();
    outcome
        .raw_errors
        .extend(plugin_ids.iter().map(|plugin_id| {
            plugin_import_raw_error(cwd, failure_stage, message.clone(), Some(plugin_id.clone()))
        }));
}

pub(super) fn plugin_import_raw_error(
    cwd: Option<&Path>,
    failure_stage: &'static str,
    message: String,
    source: Option<String>,
) -> ExternalAgentConfigImportRawError {
    ExternalAgentConfigImportRawError {
        item_type: crate::model::ExternalAgentConfigMigrationItemType::Plugins,
        error_type: None,
        sub_error_type: None,
        failure_stage: failure_stage.to_string(),
        message,
        cwd: cwd.map(Path::to_path_buf),
        source,
    }
}
