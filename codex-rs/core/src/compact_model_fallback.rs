use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;

/// Retries failures that may be model-specific and succeed with a different model.
pub(crate) fn should_retry_with_current_model(error: &CodexErr) -> bool {
    matches!(
        error.details(),
        CodexErrorDetails::InvalidRequest(_)
            | CodexErrorDetails::UnexpectedStatus(_)
            | CodexErrorDetails::ContextWindowExceeded
            | CodexErrorDetails::UsageLimitReached(_)
            | CodexErrorDetails::ServerOverloaded
            | CodexErrorDetails::InternalServerError
            | CodexErrorDetails::RetryLimit(_)
    )
}
