use std::sync::Arc;
use std::time::Duration;

use agentx_api::AuthProvider;
use agentx_api::SharedAuthProvider;
use http::HeaderMap;
use http::HeaderName;
use http::HeaderValue;
use reqwest::StatusCode;
use serde::Deserialize;
use tokio::time::sleep;
use tokio_tungstenite::connect_async;
use tracing::debug;
use tracing::info;
use tracing::warn;

use agentx_utils_rustls_provider::ensure_rustls_crypto_provider;

use crate::EnvironmentRegistryRegistrationRequest;
use crate::EnvironmentRegistryRegistrationResponse;
use crate::ExecServerError;
use crate::ExecServerRuntimePaths;
use crate::connection::JsonRpcConnection;
use crate::server::ConnectionProcessor;

const ERROR_BODY_PREVIEW_BYTES: usize = 4096;

#[derive(Clone)]
struct EnvironmentRegistryClient {
    base_url: String,
    auth_provider: SharedAuthProvider,
    http: reqwest::Client,
}

impl std::fmt::Debug for EnvironmentRegistryClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EnvironmentRegistryClient")
            .field("base_url", &self.base_url)
            .field("auth_provider", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl EnvironmentRegistryClient {
    fn new(base_url: String, auth_provider: SharedAuthProvider) -> Result<Self, ExecServerError> {
        let base_url = normalize_base_url(base_url)?;
        Ok(Self {
            base_url,
            auth_provider,
            http: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
        })
    }

    async fn register_environment(
        &self,
        environment_id: &str,
    ) -> Result<EnvironmentRegistryRegistrationResponse, ExecServerError> {
        let response = self
            .http
            .post(endpoint_url(
                &self.base_url,
                &format!("/agentx/environment/{environment_id}/register"),
            ))
            .headers(self.auth_provider.to_auth_headers())
            .json(&EnvironmentRegistryRegistrationRequest {
                id: environment_id.to_string(),
                // public_key is a wire-schema placeholder retained from the
                // pre-fork Noise design where it carried the executor's
                // ed25519 SSH pubkey. In agentx (plaintext), neither side
                // uses it. We send an empty string; agentserver-side
                // `agentx_register.go` (Part 2 of the agentx migration)
                // accepts the field as opaque and does not validate
                // contents. If a future revision adds an authenticated
                // session-binding mechanism, populate this with a real key
                // and add server-side validation in lockstep.
                public_key: String::new(),
            })
            .send()
            .await?;
        self.parse_json_response(response).await
    }

    async fn parse_json_response<R>(
        &self,
        response: reqwest::Response,
    ) -> Result<R, ExecServerError>
    where
        R: for<'de> Deserialize<'de>,
    {
        if response.status().is_success() {
            return response.json::<R>().await.map_err(ExecServerError::from);
        }

        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(environment_registry_auth_error(status, &body));
        }

        Err(environment_registry_http_error(status, &body))
    }
}

#[derive(Clone)]
struct StaticBearerAuthProvider {
    authorization: HeaderValue,
    chatgpt_account_id: Option<HeaderValue>,
}

impl std::fmt::Debug for StaticBearerAuthProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticBearerAuthProvider")
            .field("authorization", &"<redacted>")
            .field(
                "chatgpt_account_id",
                &self.chatgpt_account_id.as_ref().map(|_| "<redacted>"),
            )
            .finish()
    }
}

impl AuthProvider for StaticBearerAuthProvider {
    fn add_auth_headers(&self, headers: &mut HeaderMap) {
        headers.insert(http::header::AUTHORIZATION, self.authorization.clone());
        if let Some(chatgpt_account_id) = &self.chatgpt_account_id {
            headers.insert(
                HeaderName::from_static("chatgpt-account-id"),
                chatgpt_account_id.clone(),
            );
        }
    }
}

/// Configuration for registering an exec-server for remote use.
#[derive(Clone)]
pub struct RemoteEnvironmentConfig {
    pub base_url: String,
    pub environment_id: String,
    pub name: String,
    auth_provider: SharedAuthProvider,
}

impl std::fmt::Debug for RemoteEnvironmentConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemoteEnvironmentConfig")
            .field("base_url", &self.base_url)
            .field("environment_id", &self.environment_id)
            .field("name", &self.name)
            .field("auth_provider", &"<redacted>")
            .finish()
    }
}

impl RemoteEnvironmentConfig {
    pub fn new(
        base_url: String,
        environment_id: String,
        auth_provider: SharedAuthProvider,
    ) -> Result<Self, ExecServerError> {
        let environment_id = normalize_environment_id(environment_id)?;
        Ok(Self {
            base_url,
            environment_id,
            name: "agentx-exec-server".to_string(),
            auth_provider,
        })
    }
}

/// Register an exec-server for remote use and serve requests over plaintext WebSocket.
///
/// Connects to the agentx gateway, registers, and serves JSON-RPC over the
/// returned WebSocket URL. Reconnects with exponential backoff on disconnect.
#[tracing::instrument(
    name = "agentx.exec_server",
    skip_all,
    fields(otel.kind = "internal")
)]
pub async fn run_remote_environment(
    config: RemoteEnvironmentConfig,
    runtime_paths: ExecServerRuntimePaths,
) -> Result<(), ExecServerError> {
    ensure_rustls_crypto_provider();
    let client =
        EnvironmentRegistryClient::new(config.base_url.clone(), config.auth_provider.clone())?;
    let processor = ConnectionProcessor::new(runtime_paths);
    let mut backoff = Duration::from_secs(1);
    let mut registration = client.register_environment(&config.environment_id).await?;

    loop {
        info!(executor_id = %registration.executor_id, "Connecting to agentx gateway");
        match connect_async(registration.url.as_str()).await {
            Ok((websocket, _)) => {
                backoff = Duration::from_secs(1);
                info!(
                    executor_id = %registration.executor_id,
                    "agentx exec-server WebSocket connected"
                );
                let connection =
                    JsonRpcConnection::from_websocket(websocket, registration.executor_id.clone());
                processor.run_connection(connection).await;
                debug!("agentx exec-server connection closed; reconnecting");
            }
            Err(error) => {
                let registration_rejected = matches!(
                    &error,
                    tokio_tungstenite::tungstenite::Error::Http(response)
                        if response.status().is_client_error()
                );
                warn!(
                    executor_id = %registration.executor_id,
                    error = %error,
                    "agentx exec-server failed to connect to gateway WebSocket"
                );
                if registration_rejected {
                    registration = client.register_environment(&config.environment_id).await?;
                }
            }
        }

        sleep(backoff).await;
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

fn normalize_environment_id(environment_id: String) -> Result<String, ExecServerError> {
    let environment_id = environment_id.trim().to_string();
    if environment_id.is_empty() {
        return Err(ExecServerError::EnvironmentRegistryConfig(
            "environment id is required for remote exec-server registration".to_string(),
        ));
    }
    Ok(environment_id)
}

#[derive(Deserialize)]
struct RegistryErrorBody {
    error: Option<RegistryError>,
}

#[derive(Deserialize)]
struct RegistryError {
    code: Option<String>,
    message: Option<String>,
}

fn normalize_base_url(base_url: String) -> Result<String, ExecServerError> {
    let trimmed = base_url.trim().trim_end_matches('/').to_string();
    if trimmed.is_empty() {
        return Err(ExecServerError::EnvironmentRegistryConfig(
            "environment registry base URL is required".to_string(),
        ));
    }
    Ok(trimmed)
}

fn endpoint_url(base_url: &str, path: &str) -> String {
    format!("{base_url}/{}", path.trim_start_matches('/'))
}

fn environment_registry_auth_error(status: StatusCode, body: &str) -> ExecServerError {
    let message = registry_error_message(body).unwrap_or_else(|| "empty error body".to_string());
    ExecServerError::EnvironmentRegistryAuth(format!(
        "environment registry authentication failed ({status}): {message}"
    ))
}

fn environment_registry_http_error(status: StatusCode, body: &str) -> ExecServerError {
    let parsed = serde_json::from_str::<RegistryErrorBody>(body).ok();
    let (code, message) = parsed
        .and_then(|body| body.error)
        .map(|error| {
            (
                error.code,
                error.message.unwrap_or_else(|| {
                    preview_error_body(body).unwrap_or_else(|| "empty error body".to_string())
                }),
            )
        })
        .unwrap_or_else(|| {
            (
                None,
                preview_error_body(body)
                    .unwrap_or_else(|| "empty or malformed error body".to_string()),
            )
        });
    ExecServerError::EnvironmentRegistryHttp {
        status,
        code,
        message,
    }
}

fn registry_error_message(body: &str) -> Option<String> {
    serde_json::from_str::<RegistryErrorBody>(body)
        .ok()
        .and_then(|body| body.error)
        .and_then(|error| error.message)
        .or_else(|| preview_error_body(body))
}

fn preview_error_body(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.chars().take(ERROR_BODY_PREVIEW_BYTES).collect())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use agentx_api::AuthProvider;
    use http::HeaderMap;
    use http::HeaderValue;
    use wiremock::Mock;
    use wiremock::MockServer;
    use wiremock::ResponseTemplate;
    use wiremock::matchers::header;
    use wiremock::matchers::method;
    use wiremock::matchers::path;

    use super::*;

    #[derive(Debug)]
    struct StaticRegistryAuthProvider;

    impl AuthProvider for StaticRegistryAuthProvider {
        fn add_auth_headers(&self, headers: &mut HeaderMap) {
            let _ = headers.insert(
                http::header::AUTHORIZATION,
                HeaderValue::from_static("Bearer registry-token"),
            );
            let _ = headers.insert(
                "ChatGPT-Account-ID",
                HeaderValue::from_static("workspace-123"),
            );
        }
    }

    fn static_registry_auth_provider() -> SharedAuthProvider {
        Arc::new(StaticRegistryAuthProvider)
    }

    #[tokio::test]
    async fn register_environment_posts_with_auth_provider_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/agentx/environment/environment-requested/register"))
            .and(header("authorization", "Bearer registry-token"))
            .and(header("chatgpt-account-id", "workspace-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "executor_id": "exe-1",
                "url": "wss://gateway.test/agentx/exe-1?token=abc",
            })))
            .mount(&server)
            .await;
        let client = EnvironmentRegistryClient::new(server.uri(), static_registry_auth_provider())
            .expect("client");

        let response = client
            .register_environment("environment-requested")
            .await
            .expect("register environment");

        assert_eq!(response.executor_id, "exe-1");
        assert_eq!(response.url, "wss://gateway.test/agentx/exe-1?token=abc");
    }

    #[tokio::test]
    async fn register_environment_does_not_follow_redirects_with_auth_headers() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/agentx/environment/environment-requested/register"))
            .and(header("authorization", "Bearer registry-token"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("location", format!("{}/redirect-target", server.uri())),
            )
            .mount(&server)
            .await;
        Mock::given(path("/redirect-target"))
            .and(header("authorization", "Bearer registry-token"))
            .respond_with(ResponseTemplate::new(200))
            .expect(0)
            .mount(&server)
            .await;
        let client = EnvironmentRegistryClient::new(server.uri(), static_registry_auth_provider())
            .expect("client");

        let error = client
            .register_environment("environment-requested")
            .await
            .expect_err("redirect response should not be followed");

        assert!(matches!(
            error,
            ExecServerError::EnvironmentRegistryHttp {
                status: StatusCode::FOUND,
                ..
            }
        ));
    }

    #[test]
    fn debug_output_redacts_auth_provider() {
        let config = RemoteEnvironmentConfig::new(
            "https://registry.example".to_string(),
            "env-1".to_string(),
            static_registry_auth_provider(),
        )
        .expect("config");

        let debug = format!("{config:?}");

        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("workspace-123"));
    }

    #[test]
    fn remote_environment_config_rejects_empty_environment_id() {
        let result = RemoteEnvironmentConfig::new(
            "https://registry.example".to_string(),
            "   ".to_string(),
            static_registry_auth_provider(),
        );
        assert!(result.is_err());
    }
}
