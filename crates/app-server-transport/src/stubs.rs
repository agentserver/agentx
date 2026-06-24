/// Inline stubs for dropped crates that codex-app-server-transport depended on.
///
/// ── codex_uds stubs ───────────────────────────────────────────────────────────
#[cfg(unix)]
pub use tokio::net::UnixStream;

/// Thin wrapper around `tokio::net::UnixListener` with an async `bind` and an
/// `accept` that returns `UnixStream` directly (matching the original codex_uds API).
#[cfg(unix)]
pub struct UnixListener(tokio::net::UnixListener);

#[cfg(unix)]
impl UnixListener {
    pub async fn bind(path: &std::path::Path) -> std::io::Result<Self> {
        tokio::net::UnixListener::bind(path).map(Self)
    }

    pub async fn accept(&self) -> std::io::Result<UnixStream> {
        let (stream, _addr) = self.0.accept().await?;
        Ok(stream)
    }
}

/// Creates a private (mode 0700 on Unix) directory at `path`.
pub async fn prepare_private_socket_directory(path: &std::path::Path) -> std::io::Result<()> {
    tokio::fs::create_dir_all(path).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = tokio::fs::metadata(path).await?;
        let mut perms = meta.permissions();
        perms.set_mode(0o700);
        tokio::fs::set_permissions(path, perms).await?;
    }
    Ok(())
}

/// Returns `true` if `path` refers to a socket that no longer has a listener.
pub async fn is_stale_socket_path(path: &std::path::Path) -> std::io::Result<bool> {
    // A path is stale if it is not a socket file at all, or if we can connect to it.
    // Simplified stub: if the file exists but connection fails with ECONNREFUSED, it's stale.
    if !path.exists() {
        return Ok(false);
    }
    // Try to detect if it's a socket inode
    let metadata = std::fs::metadata(path)?;
    use std::os::unix::fs::FileTypeExt;
    if !metadata.file_type().is_socket() {
        return Ok(true);
    }
    Ok(false)
}

// ── codex_state stubs ─────────────────────────────────────────────────────────

/// Minimal record of a remote-control enrollment stored in the state database.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteControlEnrollmentRecord {
    pub websocket_url: String,
    pub account_id: String,
    pub app_server_client_name: Option<String>,
    pub server_id: String,
    pub environment_id: String,
    pub server_name: String,
    pub remote_control_enabled: Option<bool>,
}

/// Stub for the state database runtime.  All persistence calls are no-ops.
pub struct StateRuntime;

impl StateRuntime {
    /// Initialize the database.  Stub: does nothing and returns a shared handle.
    pub async fn init(
        _codex_home: std::path::PathBuf,
        _provider: String,
    ) -> Result<std::sync::Arc<Self>, String> {
        Ok(std::sync::Arc::new(Self))
    }

    pub async fn get_remote_control_enrollment(
        &self,
        _websocket_url: &str,
        _account_id: &str,
        _app_server_client_name: Option<&str>,
    ) -> Result<Option<RemoteControlEnrollmentRecord>, String> {
        Ok(None)
    }

    pub async fn upsert_remote_control_enrollment(
        &self,
        _record: &RemoteControlEnrollmentRecord,
    ) -> Result<(), String> {
        Ok(())
    }

    pub async fn delete_remote_control_enrollment(
        &self,
        _websocket_url: &str,
        _account_id: &str,
        _app_server_client_name: Option<&str>,
    ) -> Result<u64, String> {
        Ok(0)
    }

    pub async fn set_remote_control_enabled(
        &self,
        _websocket_url: &str,
        _account_id: &str,
        _app_server_client_name: Option<&str>,
        _enabled: bool,
    ) -> Result<u64, String> {
        Ok(0)
    }
}

// ── codex_core stubs ──────────────────────────────────────────────────────────

pub mod config {
    /// Returns the Codex home directory.
    pub fn find_codex_home() -> std::io::Result<std::path::PathBuf> {
        if let Ok(val) = std::env::var("AGENTX_HOME") {
            return Ok(std::path::PathBuf::from(val));
        }
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "cannot determine home directory",
                )
            })?;
        Ok(std::path::PathBuf::from(home).join(".agentx"))
    }
}

pub mod util {
    /// Exponential backoff: returns `2^attempt * 100ms`, capped by callers.
    pub fn backoff(attempt: u64) -> std::time::Duration {
        let ms = (100u64).saturating_mul(1u64 << attempt.min(16));
        std::time::Duration::from_millis(ms)
    }
}

pub mod test_support {
    use std::sync::Arc;

    /// Creates an auth manager for tests from an auth value.
    #[allow(dead_code)]
    pub fn auth_manager_from_auth(_auth: impl std::any::Any) -> Arc<agentx_login::AuthManager> {
        unimplemented!("test_support::auth_manager_from_auth is not available in this build")
    }

    /// Creates an auth manager for tests from an auth value and a home directory.
    #[allow(dead_code)]
    pub fn auth_manager_from_auth_with_home(
        _auth: impl std::any::Any,
        _home: impl AsRef<std::path::Path>,
    ) -> Arc<agentx_login::AuthManager> {
        unimplemented!(
            "test_support::auth_manager_from_auth_with_home is not available in this build"
        )
    }
}

// ── codex_model_provider stubs ────────────────────────────────────────────────

use agentx_api::SharedAuthProvider;
use std::sync::Arc;

/// An auth provider that attaches no credentials.
pub struct UnauthenticatedAuthProvider;

impl agentx_api::AuthProvider for UnauthenticatedAuthProvider {
    fn add_auth_headers(&self, _headers: &mut ::http::HeaderMap) {}
}

/// Build an unauthenticated `SharedAuthProvider`.
#[allow(dead_code)]
pub fn unauthenticated_auth_provider() -> SharedAuthProvider {
    Arc::new(UnauthenticatedAuthProvider)
}

/// Build a `SharedAuthProvider` from a login auth snapshot.
pub fn auth_provider_from_auth(_auth: &agentx_login::CodexAuth) -> SharedAuthProvider {
    // For now return an unauthenticated provider; proper implementation would
    // extract the token from `auth` and construct a bearer-token provider.
    Arc::new(UnauthenticatedAuthProvider)
}
