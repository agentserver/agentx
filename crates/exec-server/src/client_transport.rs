use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;
use tokio::process::Command;
use tokio::time::timeout;
use tokio_tungstenite::connect_async;
use tracing::debug;
use tracing::warn;

use agentx_utils_rustls_provider::ensure_rustls_crypto_provider;

use crate::ExecServerClient;
use crate::ExecServerError;
use crate::client_api::ExecServerClientConnectOptions;
use crate::client_api::RemoteExecServerConnectArgs;
use crate::client_api::StdioExecServerCommand;
use crate::client_api::StdioExecServerConnectArgs;
use crate::connection::JsonRpcConnection;

const ENVIRONMENT_CLIENT_NAME: &str = "codex-environment";

/// Reopens the transport for one logical exec-server client session.
///
/// URL connections reuse their configured endpoint.
#[derive(Clone)]
pub(crate) enum ExecServerReconnectStrategy {
    WebSocket(RemoteExecServerConnectArgs),
}

impl ExecServerReconnectStrategy {
    pub(crate) async fn resume(
        &self,
        session_id: &str,
    ) -> Result<(JsonRpcConnection, ExecServerClientConnectOptions), ExecServerError> {
        match self {
            Self::WebSocket(args) => {
                let mut args = args.clone();
                args.resume_session_id = Some(session_id.to_string());
                let connection = ExecServerClient::open_websocket_connection(&args).await?;
                Ok((connection, args.into()))
            }
        }
    }
}

impl ExecServerClient {
    /// Open the selected transport and run the common JSON-RPC initialization.
    pub(crate) async fn connect_for_transport(
        transport_params: crate::client_api::ExecServerTransportParams,
    ) -> Result<Self, ExecServerError> {
        match transport_params {
            crate::client_api::ExecServerTransportParams::WebSocketUrl {
                websocket_url,
                connect_timeout,
                initialize_timeout,
            } => {
                Self::connect_websocket(RemoteExecServerConnectArgs {
                    websocket_url,
                    client_name: ENVIRONMENT_CLIENT_NAME.to_string(),
                    connect_timeout,
                    initialize_timeout,
                    resume_session_id: None,
                })
                .await
            }
            crate::client_api::ExecServerTransportParams::StdioCommand {
                command,
                initialize_timeout,
            } => {
                Self::connect_stdio_command(StdioExecServerConnectArgs {
                    command,
                    client_name: ENVIRONMENT_CLIENT_NAME.to_string(),
                    initialize_timeout,
                    resume_session_id: None,
                })
                .await
            }
        }
    }

    pub async fn connect_websocket(
        args: RemoteExecServerConnectArgs,
    ) -> Result<Self, ExecServerError> {
        let connection = Self::open_websocket_connection(&args).await?;
        let options = args.clone().into();
        Self::connect_with_recovery(
            connection,
            options,
            Some(ExecServerReconnectStrategy::WebSocket(args)),
        )
        .await
    }

    pub(crate) async fn open_websocket_connection(
        args: &RemoteExecServerConnectArgs,
    ) -> Result<JsonRpcConnection, ExecServerError> {
        ensure_rustls_crypto_provider();
        let websocket_url = args.websocket_url.clone();
        let connect_timeout = args.connect_timeout;
        let (stream, _) = timeout(connect_timeout, connect_async(websocket_url.as_str()))
            .await
            .map_err(|_| ExecServerError::WebSocketConnectTimeout {
                url: websocket_url.clone(),
                timeout: connect_timeout,
            })?
            .map_err(|source| ExecServerError::WebSocketConnect {
                url: websocket_url.clone(),
                source,
            })?;

        let connection_label = format!("exec-server websocket {websocket_url}");
        let connection = JsonRpcConnection::from_websocket(stream, connection_label);
        Ok(connection)
    }

    pub(crate) async fn connect_stdio_command(
        args: StdioExecServerConnectArgs,
    ) -> Result<Self, ExecServerError> {
        let mut child = stdio_command_process(&args.command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(ExecServerError::Spawn)?;

        let stdin = child.stdin.take().ok_or_else(|| {
            ExecServerError::Protocol("spawned exec-server command has no stdin".to_string())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            ExecServerError::Protocol("spawned exec-server command has no stdout".to_string())
        })?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(async move {
                let mut lines = BufReader::new(stderr).lines();
                loop {
                    match lines.next_line().await {
                        Ok(Some(line)) => debug!("exec-server stdio stderr: {line}"),
                        Ok(None) => break,
                        Err(err) => {
                            warn!("failed to read exec-server stdio stderr: {err}");
                            break;
                        }
                    }
                }
            });
        }

        Self::connect(
            JsonRpcConnection::from_stdio(stdout, stdin, "exec-server stdio command".to_string())
                .with_child_process(child),
            args.into(),
        )
        .await
    }
}

fn stdio_command_process(stdio_command: &StdioExecServerCommand) -> Command {
    let mut command = Command::new(&stdio_command.program);
    command.args(&stdio_command.args);
    command.envs(&stdio_command.env);
    if let Some(cwd) = &stdio_command.cwd {
        command.current_dir(cwd);
    }
    #[cfg(unix)]
    command.process_group(0);
    command
}
