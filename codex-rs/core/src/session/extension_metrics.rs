use std::sync::Arc;

use codex_extension_api::ExtensionMetrics;

#[derive(Clone, Debug, Default)]
struct NoopExtensionMetrics;

impl ExtensionMetrics for NoopExtensionMetrics {
    fn histogram(&self, _name: &str, _value: i64, _tags: &[(&str, &str)]) {}
}

pub(crate) fn noop_extension_metrics() -> Arc<dyn ExtensionMetrics> {
    Arc::new(NoopExtensionMetrics)
}
