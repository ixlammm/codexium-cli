//! Reusable HTTP client and request-builder wrappers.

use http::Error as HttpRequestBuildError;
use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use reqwest::IntoUrl;
use reqwest::Method;
use serde::Serialize;
use std::fmt::Display;
use std::time::Duration;

pub type HttpError = reqwest::Error;
pub type HttpResponse = reqwest::Response;

/// Reusable HTTP client wrapper with shared tracing and request-diagnostic behavior.
///
/// Product callers should obtain this through [`crate::HttpClientFactory`] for a fixed
/// destination or use [`crate::RouteAwareClientPool`] when request and redirect URLs can vary.
#[derive(Clone, Debug)]
pub struct HttpClient {
    inner: reqwest::Client,
    request_logging: RequestLogging,
}

impl HttpClient {
    pub fn new(inner: reqwest::Client) -> Self {
        Self::from_parts(inner, RequestLogging::Enabled)
    }

    /// Creates a client that suppresses request URL and response-header diagnostics.
    ///
    /// Use this for endpoints whose URLs or headers may contain credentials that are redacted by
    /// the caller above the HTTP transport boundary.
    pub fn new_without_request_logging(inner: reqwest::Client) -> Self {
        Self::from_parts(inner, RequestLogging::Disabled)
    }

    pub(crate) fn from_parts(inner: reqwest::Client, request_logging: RequestLogging) -> Self {
        Self {
            inner,
            request_logging,
        }
    }

    pub fn get<U>(&self, url: U) -> RequestBuilder
    where
        U: IntoUrl,
    {
        self.request(Method::GET, url)
    }

    pub fn head<U>(&self, url: U) -> RequestBuilder
    where
        U: IntoUrl,
    {
        self.request(Method::HEAD, url)
    }

    pub fn post<U>(&self, url: U) -> RequestBuilder
    where
        U: IntoUrl,
    {
        self.request(Method::POST, url)
    }

    pub fn delete<U>(&self, url: U) -> RequestBuilder
    where
        U: IntoUrl,
    {
        self.request(Method::DELETE, url)
    }

    pub fn request<U>(&self, method: Method, url: U) -> RequestBuilder
    where
        U: IntoUrl,
    {
        let url_str = url.as_str().to_string();
        RequestBuilder::new(
            self.inner.request(method.clone(), url),
            method,
            url_str,
            self.request_logging,
        )
    }

    pub(crate) async fn execute(
        &self,
        request: reqwest::Request,
    ) -> Result<reqwest::Response, reqwest::Error> {
        let method = request.method().clone();
        let url = request.url().to_string();

        match self.execute_without_request_logging(request).await {
            Ok(response) => {
                self.log_response(&method, &url, &response);
                Ok(response)
            }
            Err(error) => {
                self.log_error(&method, &url, &error);
                Err(error)
            }
        }
    }

    pub(crate) async fn execute_without_request_logging(
        &self,
        request: reqwest::Request,
    ) -> Result<reqwest::Response, reqwest::Error> {
        self.inner.execute(request).await
    }

    pub(crate) fn log_response(&self, method: &Method, url: &str, response: &reqwest::Response) {
        if self.request_logging == RequestLogging::Enabled {
            tracing::debug!(
                method = %method,
                url = %url,
                status = %response.status(),
                headers = ?response.headers(),
                version = ?response.version(),
                "Request completed"
            );
        }
    }

    pub(crate) fn log_error(&self, method: &Method, url: &str, error: &reqwest::Error) {
        if self.request_logging == RequestLogging::Enabled {
            tracing::debug!(
                method = %method,
                url = %url,
                status = error.status().map(|status| status.as_u16()),
                error = %error,
                "Request failed"
            );
        }
    }
    pub(crate) fn log_error_summary(&self, method: &Method, url: &str, error: &reqwest::Error) {
        if self.request_logging == RequestLogging::Enabled {
            tracing::debug!(
                method = %method,
                url = %url,
                status = error.status().map(|status| status.as_u16()),
                is_timeout = error.is_timeout(),
                is_connect = error.is_connect(),
                "Request failed"
            );
        }
    }

    pub(crate) const fn request_logging_enabled(&self) -> bool {
        matches!(self.request_logging, RequestLogging::Enabled)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum RequestLogging {
    #[default]
    Enabled,
    Disabled,
}

#[must_use = "requests are not sent unless `send` is awaited"]
#[derive(Debug)]
pub struct RequestBuilder {
    builder: reqwest::RequestBuilder,
    method: Method,
    url: String,
    request_logging: RequestLogging,
}

impl RequestBuilder {
    fn new(
        builder: reqwest::RequestBuilder,
        method: Method,
        url: String,
        request_logging: RequestLogging,
    ) -> Self {
        Self {
            builder,
            method,
            url,
            request_logging,
        }
    }

    fn map(self, f: impl FnOnce(reqwest::RequestBuilder) -> reqwest::RequestBuilder) -> Self {
        Self {
            builder: f(self.builder),
            method: self.method,
            url: self.url,
            request_logging: self.request_logging,
        }
    }

    pub fn headers(self, headers: HeaderMap) -> Self {
        self.map(|builder| builder.headers(headers))
    }

    pub fn header<K, V>(self, key: K, value: V) -> Self
    where
        HeaderName: TryFrom<K>,
        <HeaderName as TryFrom<K>>::Error: Into<HttpRequestBuildError>,
        HeaderValue: TryFrom<V>,
        <HeaderValue as TryFrom<V>>::Error: Into<HttpRequestBuildError>,
    {
        self.map(|builder| builder.header(key, value))
    }

    pub fn bearer_auth<T>(self, token: T) -> Self
    where
        T: Display,
    {
        self.map(|builder| builder.bearer_auth(token))
    }

    pub fn timeout(self, timeout: Duration) -> Self {
        self.map(|builder| builder.timeout(timeout))
    }

    pub fn json<T>(self, value: &T) -> Self
    where
        T: ?Sized + Serialize,
    {
        self.map(|builder| builder.json(value))
    }

    pub fn query<T>(self, query: &T) -> Self
    where
        T: ?Sized + Serialize,
    {
        self.map(|builder| builder.query(query))
    }

    pub fn body<B>(self, body: B) -> Self
    where
        B: Into<reqwest::Body>,
    {
        self.map(|builder| builder.body(body))
    }

    pub async fn send(self) -> Result<HttpResponse, HttpError> {
        match self.builder.send().await {
            Ok(response) => {
                if self.request_logging == RequestLogging::Enabled {
                    tracing::debug!(
                        method = %self.method,
                        url = %self.url,
                        status = %response.status(),
                        headers = ?response.headers(),
                        version = ?response.version(),
                        "Request completed"
                    );
                }

                Ok(response)
            }
            Err(error) => {
                if self.request_logging == RequestLogging::Enabled {
                    let status = error.status();
                    tracing::debug!(
                        method = %self.method,
                        url = %self.url,
                        status = status.map(|s| s.as_u16()),
                        error = %error,
                        "Request failed"
                    );
                }
                Err(error)
            }
        }
    }
}
