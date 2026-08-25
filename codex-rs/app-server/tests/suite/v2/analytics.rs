use anyhow::Result;
use app_test_support::ChatGptAuthFixture;
use app_test_support::write_chatgpt_auth;
use codex_config::types::AuthCredentialsStoreMode;
use std::path::Path;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

pub(crate) async fn mount_analytics_capture(server: &MockServer, codex_home: &Path) -> Result<()> {
    Mock::given(method("POST"))
        .and(path("/codex/analytics-events/events"))
        .respond_with(ResponseTemplate::new(200))
        .mount(server)
        .await;

    write_chatgpt_auth(
        codex_home,
        ChatGptAuthFixture::new("chatgpt-token")
            .account_id("account-123")
            .chatgpt_user_id("user-123")
            .chatgpt_account_id("account-123"),
        AuthCredentialsStoreMode::File,
    )?;

    Ok(())
}
