/// Inline stubs for dropped crates that codex-login depended on.
/// These provide enough surface to compile; real implementations are not needed
/// because the agentx authentication path differs from the original Codex path.

// ── codex_config stubs ────────────────────────────────────────────────────────

/// Where authentication credentials are persisted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthCredentialsStoreMode {
    /// Store in a plain JSON file (default).
    #[default]
    File,
    /// Store in the OS keyring directly.
    Keyring,
    /// Try keyring, fall back to file.
    Auto,
    /// Keep only in memory; do not persist.
    Ephemeral,
}

/// Which keyring backend to use when `AuthCredentialsStoreMode` is keyring-based.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AuthKeyringBackendKind {
    /// Use the OS keyring directly.
    #[default]
    Direct,
    /// Use the Secrets Service (GNOME Keyring / KWallet).
    Secrets,
}

/// Regional residency hint for API requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResidencyRequirement {
    Us,
}

// ── codex_terminal_detection stub ─────────────────────────────────────────────

/// Returns a platform user-agent fragment.  Stub always returns empty string.
pub fn user_agent() -> String {
    String::new()
}

// ── codex_keyring_store stubs ─────────────────────────────────────────────────

/// Error type returned by keyring operations.
#[derive(Debug)]
pub struct KeyringError(String);

impl KeyringError {
    pub fn message(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for KeyringError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Trait for loading and saving opaque string secrets to a system keyring.
pub trait KeyringStore: Send + Sync + std::fmt::Debug {
    fn load(&self, service: &str, key: &str) -> Result<Option<String>, KeyringError>;
    fn save(&self, service: &str, key: &str, value: &str) -> Result<(), KeyringError>;
    fn delete(&self, service: &str, key: &str) -> Result<bool, KeyringError>;
}

/// Default in-process file-backed keyring store (stub: always acts as if empty).
#[derive(Debug, Clone, Default)]
pub struct DefaultKeyringStore;

impl KeyringStore for DefaultKeyringStore {
    fn load(&self, _service: &str, _key: &str) -> Result<Option<String>, KeyringError> {
        Ok(None)
    }
    fn save(&self, _service: &str, _key: &str, _value: &str) -> Result<(), KeyringError> {
        Ok(())
    }
    fn delete(&self, _service: &str, _key: &str) -> Result<bool, KeyringError> {
        Ok(false)
    }
}

// ── codex_secrets stubs ───────────────────────────────────────────────────────

/// A validated secret name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SecretName(String);

impl SecretName {
    pub fn new(name: impl Into<String>) -> Result<Self, String> {
        Ok(Self(name.into()))
    }
}

/// Scope for a stored secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretScope {
    Global,
}

/// Backend kind for the secrets manager.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretsBackendKind {
    Local,
}

/// Namespace within local secrets storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSecretsNamespace {
    CodexAuth,
}

/// Manages secrets using a backing keyring store.
/// Stub: always reads/writes nothing successfully.
#[derive(Clone, Debug)]
pub struct SecretsManager;

impl SecretsManager {
    pub fn new_with_keyring_store_and_namespace(
        _codex_home: std::path::PathBuf,
        _backend: SecretsBackendKind,
        _keyring_store: std::sync::Arc<dyn KeyringStore>,
        _namespace: LocalSecretsNamespace,
    ) -> Self {
        Self
    }

    pub fn get(&self, _scope: &SecretScope, _name: &SecretName) -> Result<Option<String>, String> {
        Ok(None)
    }

    pub fn set(
        &self,
        _scope: &SecretScope,
        _name: &SecretName,
        _value: &str,
    ) -> Result<(), String> {
        Ok(())
    }

    pub fn delete(&self, _scope: &SecretScope, _name: &SecretName) -> Result<bool, String> {
        Ok(false)
    }
}

// ── codex_utils_template stub ─────────────────────────────────────────────────

/// Simple `{{key}}` string template.
pub struct Template {
    source: String,
}

impl Template {
    pub fn parse(source: &str) -> Result<Self, String> {
        Ok(Self {
            source: source.to_string(),
        })
    }

    /// Replace `{{key}}` placeholders with values from the iterator.
    pub fn render<'a, I>(&self, pairs: I) -> Result<String, String>
    where
        I: IntoIterator<Item = (&'a str, String)>,
    {
        let mut out = self.source.clone();
        for (key, value) in pairs {
            let placeholder = format!("{{{{{key}}}}}");
            out = out.replace(&placeholder, &value);
        }
        Ok(out)
    }
}

// ── codex_model_provider_info stubs ──────────────────────────────────────────

/// Wire protocol kind supported by a model provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireApi {
    Responses,
    Chat,
}

/// Minimal descriptor for an external model provider.
#[derive(Debug, Clone)]
pub struct ModelProviderInfo {
    pub name: String,
    pub base_url: Option<String>,
    pub env_key: Option<String>,
    pub env_key_instructions: Option<String>,
    pub experimental_bearer_token: Option<bool>,
    pub auth: Option<serde_json::Value>,
    pub aws: Option<serde_json::Value>,
    pub wire_api: WireApi,
    pub query_params: Option<serde_json::Value>,
    pub http_headers: Option<serde_json::Value>,
    pub env_http_headers: Option<serde_json::Value>,
    pub request_max_retries: Option<u32>,
    pub stream_max_retries: Option<u32>,
    pub stream_idle_timeout_ms: Option<u64>,
    pub websocket_connect_timeout_ms: Option<u64>,
    pub requires_openai_auth: bool,
    pub supports_websockets: bool,
}

// ── codex_otel stubs ──────────────────────────────────────────────────────────

/// Telemetry metadata for auth environment state.
#[derive(Debug, Clone, Default)]
pub struct AuthEnvTelemetryMetadata {
    pub openai_api_key_env_present: bool,
    pub codex_api_key_env_present: bool,
    pub codex_api_key_env_enabled: bool,
    pub provider_env_key_name: Option<String>,
    pub provider_env_key_present: Option<bool>,
    pub refresh_token_url_override_present: bool,
}
