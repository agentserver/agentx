use clap::Parser;
use agentx_api::SharedAuthProvider;
use agentx_arg0::Arg0DispatchPaths;
use agentx_arg0::arg0_dispatch_or_else;
use agentx_login::AuthCredentialsStoreMode;
use agentx_login::AuthKeyringBackendKind;
use agentx_login::AuthManager;
use agentx_login::AuthManagerConfig;
use agentx_login::AuthRouteConfig;
use agentx_login::CodexAuth;
use agentx_login::read_codex_access_token_from_env;
use std::path::PathBuf;
use std::sync::Arc;

mod exec_server_telemetry;

/// Minimal exec-server configuration — replaces the dropped codex_core::config::Config.
/// No TOML config loading needed; agentx only needs the ChatGPT base URL for auth.
pub(crate) struct ExecServerConfig {
    pub chatgpt_base_url: String,
}

impl Default for ExecServerConfig {
    fn default() -> Self {
        Self {
            chatgpt_base_url: "https://chatgpt.com/backend-api".to_string(),
        }
    }
}

impl AuthManagerConfig for ExecServerConfig {
    fn codex_home(&self) -> PathBuf {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".codex")
    }

    fn cli_auth_credentials_store_mode(&self) -> AuthCredentialsStoreMode {
        AuthCredentialsStoreMode::default()
    }

    fn auth_keyring_backend_kind(&self) -> AuthKeyringBackendKind {
        AuthKeyringBackendKind::default()
    }

    fn forced_chatgpt_workspace_id(&self) -> Option<Vec<String>> {
        None
    }

    fn chatgpt_base_url(&self) -> String {
        self.chatgpt_base_url.clone()
    }

    fn auth_route_config(&self) -> Option<AuthRouteConfig> {
        None
    }
}

/// agentx — Remote process / fs executor (forked from codex exec-server)
///
/// Registers this host as a remote environment with the given control-plane URL.
#[derive(Debug, Parser)]
#[clap(
    name = "agentx",
    about = "Remote process / fs executor (forked from codex exec-server)",
    author,
    version
)]
struct AgentxCli {
    #[clap(flatten)]
    exec: ExecServerCommand,
}

#[derive(Debug, Parser)]
struct ExecServerCommand {
    /// Register this exec-server as a remote environment using the given base URL.
    #[arg(long = "remote", value_name = "URL", required = true, requires = "environment_id")]
    remote: String,

    /// Environment id to attach to when registering remotely.
    #[arg(long = "environment-id", value_name = "ID", required = true)]
    environment_id: String,

    /// Human-readable environment name.
    #[arg(long = "name", value_name = "NAME")]
    name: Option<String>,

    /// Use Agent Identity auth from AGENTX_ACCESS_TOKEN for remote registration.
    #[arg(long = "use-agent-identity-auth")]
    use_agent_identity_auth: bool,
}

fn main() -> anyhow::Result<()> {
    arg0_dispatch_or_else(move |arg0_paths: Arg0DispatchPaths| async move {
        cli_main(arg0_paths).await?;
        Ok(())
    })
}

async fn cli_main(arg0_paths: Arg0DispatchPaths) -> anyhow::Result<()> {
    let AgentxCli { exec: cmd } = AgentxCli::parse();
    run_exec_server_command(cmd, &arg0_paths).await
}

async fn run_exec_server_command(
    cmd: ExecServerCommand,
    arg0_paths: &Arg0DispatchPaths,
) -> anyhow::Result<()> {
    let codex_self_exe = arg0_paths
        .codex_self_exe
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Codex executable path is not configured"))?;
    let runtime_paths = agentx_exec_server::ExecServerRuntimePaths::new(
        codex_self_exe,
        arg0_paths.codex_linux_sandbox_exe.clone(),
    )?;

    let base_url = cmd.remote;
    let environment_id = cmd.environment_id;
    let config = ExecServerConfig::default();
    let _otel = exec_server_telemetry::init(Some(&config))
        .inspect_err(|err| eprintln!("Could not create otel exporter: {err}"))
        .ok();
    let auth_provider =
        load_exec_server_remote_auth_provider(&config, &base_url, cmd.use_agent_identity_auth)
            .await?;
    let mut remote_config = agentx_exec_server::RemoteEnvironmentConfig::new(
        base_url,
        environment_id,
        auth_provider,
    )?;
    if let Some(name) = cmd.name {
        remote_config.name = name;
    }
    agentx_exec_server::run_remote_environment(remote_config, runtime_paths).await?;
    Ok(())
}

async fn load_exec_server_remote_auth_provider(
    config: &ExecServerConfig,
    base_url: &str,
    use_agent_identity_auth: bool,
) -> anyhow::Result<agentx_api::SharedAuthProvider> {
    if use_agent_identity_auth {
        let agent_identity_jwt = read_codex_access_token_from_env().ok_or_else(|| {
            anyhow::anyhow!("AGENTX_ACCESS_TOKEN is required when --use-agent-identity-auth is set")
        })?;
        let auth_route_config = config.auth_route_config();
        let auth = CodexAuth::from_agent_identity_jwt(
            &agent_identity_jwt,
            Some(&config.chatgpt_base_url),
            auth_route_config.as_ref(),
        )
        .await?;
        return Ok(auth_provider_from_auth(&auth));
    }

    let auth = load_exec_server_remote_auth(
        config,
        "remote exec-server registration requires ChatGPT authentication or API key authentication; run `agentx login` or set AGENTX_API_KEY",
    )
    .await?;

    if !is_supported_exec_server_remote_auth(&auth) {
        anyhow::bail!(
            "remote exec-server registration requires ChatGPT authentication or API key authentication; Agent Identity auth requires --use-agent-identity-auth"
        );
    }

    if auth.is_api_key_auth() {
        validate_api_key_remote_host(base_url)?;
    }

    Ok(auth_provider_from_auth(&auth))
}

fn is_supported_exec_server_remote_auth(auth: &CodexAuth) -> bool {
    auth.is_chatgpt_auth() || auth.is_api_key_auth()
}

fn validate_api_key_remote_host(base_url: &str) -> anyhow::Result<()> {
    let url = url::Url::parse(base_url)
        .map_err(|err| anyhow::anyhow!("invalid remote exec-server registration URL: {err}"))?;
    let host = url.host().ok_or_else(|| {
        anyhow::anyhow!("remote exec-server registration URL must include a host")
    })?;

    let is_loopback = match &host {
        url::Host::Domain(host) => host.eq_ignore_ascii_case("localhost"),
        url::Host::Ipv4(ip) => ip.is_loopback(),
        url::Host::Ipv6(ip) => ip.is_loopback(),
    };
    let is_openai_host = match &host {
        url::Host::Domain(host) => ["openai.com", "openai.org"].into_iter().any(|domain| {
            host.eq_ignore_ascii_case(domain)
                || host.to_ascii_lowercase().ends_with(&format!(".{domain}"))
        }),
        _ => false,
    };
    let is_allowed = match url.scheme() {
        "https" => is_loopback || is_openai_host,
        "http" => is_loopback,
        _ => false,
    };

    if !is_allowed {
        anyhow::bail!(
            "remote exec-server API-key authentication is restricted to HTTPS openai.com and openai.org hosts and subdomains or loopback hosts"
        );
    }

    Ok(())
}

async fn load_exec_server_remote_auth(
    config: &ExecServerConfig,
    missing_auth_error: &'static str,
) -> anyhow::Result<agentx_login::CodexAuth> {
    let auth_manager =
        AuthManager::shared_from_config(config, /*enable_codex_api_key_env*/ true).await;

    let auth = match auth_manager.auth().await {
        Some(auth) => auth,
        None => {
            auth_manager.reload().await;
            auth_manager
                .auth()
                .await
                .ok_or_else(|| anyhow::anyhow!(missing_auth_error))?
        }
    };

    Ok(auth)
}

/// Build a `SharedAuthProvider` from a login auth snapshot.
///
/// Dispatches on the `CodexAuth` variant:
/// - `AgentIdentity` → `Authorization: AgentAssertion <jwt>` (signed assertion)
/// - All others (ApiKey / Chatgpt / ChatgptAuthTokens / PersonalAccessToken)
///   → `Authorization: Bearer <token>`
fn auth_provider_from_auth(auth: &CodexAuth) -> SharedAuthProvider {
    use agentx_agent_identity::AgentIdentityKey;
    use agentx_agent_identity::authorization_header_for_agent_task;

    // ------- AgentIdentity: signed AgentAssertion header -------
    struct AgentIdentityAuthProvider {
        agent_runtime_id: String,
        agent_private_key: String,
        task_id: String,
    }
    impl agentx_api::AuthProvider for AgentIdentityAuthProvider {
        fn add_auth_headers(&self, headers: &mut http::HeaderMap) {
            let key = AgentIdentityKey {
                agent_runtime_id: &self.agent_runtime_id,
                private_key_pkcs8_base64: &self.agent_private_key,
            };
            let result = authorization_header_for_agent_task(key, &self.task_id)
                .map_err(std::io::Error::other);
            if let Ok(header_value) = result
                && let Ok(header) = http::HeaderValue::from_str(&header_value)
            {
                let _ = headers.insert(http::header::AUTHORIZATION, header);
            }
        }
    }

    // ------- Bearer: ApiKey / Chatgpt / ChatgptAuthTokens / PersonalAccessToken -------
    struct BearerAuthProvider {
        token: Option<String>,
        account_id: Option<String>,
        is_fedramp_account: bool,
    }
    impl agentx_api::AuthProvider for BearerAuthProvider {
        fn add_auth_headers(&self, headers: &mut http::HeaderMap) {
            if let Some(token) = self.token.as_ref()
                && let Ok(header) = http::HeaderValue::from_str(&format!("Bearer {token}"))
            {
                let _ = headers.insert(http::header::AUTHORIZATION, header);
            }
            if let Some(account_id) = self.account_id.as_ref()
                && let Ok(header) = http::HeaderValue::from_str(account_id)
            {
                let _ = headers.insert("ChatGPT-Account-ID", header);
            }
            if self.is_fedramp_account {
                let _ = headers.insert(
                    "X-OpenAI-Fedramp",
                    http::HeaderValue::from_static("true"),
                );
            }
        }
    }

    match auth {
        CodexAuth::AgentIdentity(ai) => {
            let record = ai.record();
            Arc::new(AgentIdentityAuthProvider {
                agent_runtime_id: record.agent_runtime_id.clone(),
                agent_private_key: record.agent_private_key.clone(),
                task_id: ai.run_task_id().to_string(),
            })
        }
        CodexAuth::ApiKey(_)
        | CodexAuth::Chatgpt(_)
        | CodexAuth::ChatgptAuthTokens(_)
        | CodexAuth::PersonalAccessToken(_) => Arc::new(BearerAuthProvider {
            token: auth.get_token().ok(),
            account_id: auth.get_account_id(),
            is_fedramp_account: auth.is_fedramp_account(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_server_remote_auth_accepts_api_key_auth() {
        let auth = CodexAuth::from_api_key("sk-test");

        assert!(is_supported_exec_server_remote_auth(&auth));
    }

    #[test]
    fn exec_server_remote_api_key_auth_accepts_https_openai_domains() {
        for base_url in [
            "https://openai.com/api",
            "https://service.openai.com/api",
            "https://openai.org/api",
            "https://service.openai.org/api",
        ] {
            assert!(validate_api_key_remote_host(base_url).is_ok());
        }
    }

    #[test]
    fn exec_server_remote_api_key_auth_accepts_http_loopback() {
        for base_url in [
            "http://localhost:8098/api",
            "http://127.0.0.1:8098/api",
            "http://[::1]:8098/api",
        ] {
            assert!(validate_api_key_remote_host(base_url).is_ok());
        }
    }

    #[test]
    fn exec_server_remote_api_key_auth_rejects_http_openai_domain() {
        for base_url in [
            "http://service.openai.com/api",
            "http://service.openai.org/api",
        ] {
            let error = validate_api_key_remote_host(base_url)
                .expect_err("reject plaintext OpenAI destination");

            assert_eq!(
                error.to_string(),
                "remote exec-server API-key authentication is restricted to HTTPS openai.com and openai.org hosts and subdomains or loopback hosts"
            );
        }
    }

    #[test]
    fn exec_server_remote_api_key_auth_rejects_suffix_spoof() {
        let error = validate_api_key_remote_host("https://service.openai.org.evil.example/api")
            .expect_err("reject suffix spoof");

        assert_eq!(
            error.to_string(),
            "remote exec-server API-key authentication is restricted to HTTPS openai.com and openai.org hosts and subdomains or loopback hosts"
        );
    }

    #[test]
    fn auth_provider_from_api_key_injects_bearer_header() {
        use agentx_api::AuthProvider as _;

        let auth = CodexAuth::from_api_key("sk-test-key");
        let provider = auth_provider_from_auth(&auth);
        let headers = provider.to_auth_headers();

        let authorization = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .expect("Authorization header must be present for ApiKey auth");

        assert!(
            authorization.starts_with("Bearer "),
            "ApiKey auth must produce a Bearer Authorization header, got: {authorization}"
        );
        assert!(
            authorization.contains("sk-test-key"),
            "Bearer header must contain the api key, got: {authorization}"
        );
    }

    #[test]
    fn auth_provider_from_chatgpt_auth_injects_bearer_header() {
        use agentx_api::AuthProvider as _;

        let auth = CodexAuth::create_dummy_chatgpt_auth_for_testing();
        let provider = auth_provider_from_auth(&auth);
        let headers = provider.to_auth_headers();

        let authorization = headers
            .get(http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .expect("Authorization header must be present for Chatgpt auth");

        assert!(
            authorization.starts_with("Bearer "),
            "Chatgpt auth must produce a Bearer Authorization header, got: {authorization}"
        );
    }
}
