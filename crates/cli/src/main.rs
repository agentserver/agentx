use clap::Parser;
use codex_arg0::Arg0DispatchPaths;
use codex_arg0::arg0_dispatch_or_else;
use codex_login::AuthManager;
use codex_login::CodexAuth;
use codex_login::read_codex_access_token_from_env;
use codex_utils_cli::CliConfigOverrides;

mod exec_server_telemetry;

use codex_config::LoaderOverrides;
use codex_core::config::ConfigBuilder;

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
    pub config_overrides: CliConfigOverrides,

    #[clap(flatten)]
    exec: ExecServerCommand,
}

#[derive(Debug, Parser)]
struct ExecServerCommand {
    /// Error out when config.toml contains fields that are not recognized by this version of Codex.
    #[arg(long = "strict-config", default_value_t = false)]
    strict_config: bool,

    /// Register this exec-server as a remote environment using the given base URL.
    #[arg(long = "remote", value_name = "URL", required = true, requires = "environment_id")]
    remote: String,

    /// Environment id to attach to when registering remotely.
    #[arg(long = "environment-id", value_name = "ID", required = true)]
    environment_id: String,

    /// Human-readable environment name.
    #[arg(long = "name", value_name = "NAME")]
    name: Option<String>,

    /// Use Agent Identity auth from CODEX_ACCESS_TOKEN for remote registration.
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
    let AgentxCli {
        config_overrides: root_config_overrides,
        exec: cmd,
    } = AgentxCli::parse();

    let strict_config = cmd.strict_config;
    run_exec_server_command(cmd, &arg0_paths, &root_config_overrides, strict_config).await
}

async fn run_exec_server_command(
    cmd: ExecServerCommand,
    arg0_paths: &Arg0DispatchPaths,
    root_config_overrides: &CliConfigOverrides,
    strict_config: bool,
) -> anyhow::Result<()> {
    let codex_self_exe = arg0_paths
        .codex_self_exe
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Codex executable path is not configured"))?;
    let runtime_paths = codex_exec_server::ExecServerRuntimePaths::new(
        codex_self_exe,
        arg0_paths.codex_linux_sandbox_exe.clone(),
    )?;

    let base_url = cmd.remote;
    let environment_id = cmd.environment_id;
    let config = load_exec_server_config(root_config_overrides, strict_config).await?;
    let _otel = exec_server_telemetry::init(Some(&config))
        .inspect_err(|err| eprintln!("Could not create otel exporter: {err}"))
        .ok();
    let auth_provider =
        load_exec_server_remote_auth_provider(&config, &base_url, cmd.use_agent_identity_auth)
            .await?;
    let mut remote_config = codex_exec_server::RemoteEnvironmentConfig::new(
        base_url,
        environment_id,
        auth_provider,
    )?;
    if let Some(name) = cmd.name {
        remote_config.name = name;
    }
    codex_exec_server::run_remote_environment(remote_config, runtime_paths).await?;
    Ok(())
}

async fn load_exec_server_remote_auth_provider(
    config: &codex_core::config::Config,
    base_url: &str,
    use_agent_identity_auth: bool,
) -> anyhow::Result<codex_api::SharedAuthProvider> {
    if use_agent_identity_auth {
        let agent_identity_jwt = read_codex_access_token_from_env().ok_or_else(|| {
            anyhow::anyhow!("CODEX_ACCESS_TOKEN is required when --use-agent-identity-auth is set")
        })?;
        let auth_route_config = config.auth_route_config();
        let auth = CodexAuth::from_agent_identity_jwt(
            &agent_identity_jwt,
            Some(&config.chatgpt_base_url),
            auth_route_config.as_ref(),
        )
        .await?;
        return Ok(codex_model_provider::auth_provider_from_auth(&auth));
    }

    let auth = load_exec_server_remote_auth(
        config,
        "remote exec-server registration requires ChatGPT authentication or API key authentication; run `codex login` or set CODEX_API_KEY",
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

    Ok(codex_model_provider::auth_provider_from_auth(&auth))
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

async fn load_exec_server_config(
    root_config_overrides: &CliConfigOverrides,
    strict_config: bool,
) -> anyhow::Result<codex_core::config::Config> {
    let cli_kv_overrides = root_config_overrides
        .parse_overrides()
        .map_err(anyhow::Error::msg)?;
    Ok(ConfigBuilder::default()
        .cli_overrides(cli_kv_overrides)
        .strict_config(strict_config)
        .build()
        .await?)
}

async fn load_exec_server_remote_auth(
    config: &codex_core::config::Config,
    missing_auth_error: &'static str,
) -> anyhow::Result<codex_login::CodexAuth> {
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
}
