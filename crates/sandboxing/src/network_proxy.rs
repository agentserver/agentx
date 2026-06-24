/// Minimal stubs replacing the dropped `codex_network_proxy` crate.
///
/// The original crate provided a MITM-proxy and network-decision engine used
/// by the macOS seatbelt sandbox.  In agentx this functionality is not needed
/// (we target the Linux exec-server path), so we inline just enough to satisfy
/// the type system.
use std::collections::HashMap;

use agentx_utils_absolute_path::AbsolutePathBuf;

/// Environment variable keys that may carry proxy URLs.
#[allow(dead_code)]
pub const PROXY_URL_ENV_KEYS: &[&str] = &[
    "http_proxy",
    "HTTP_PROXY",
    "https_proxy",
    "HTTPS_PROXY",
    "all_proxy",
    "ALL_PROXY",
];

/// Returns true when at least one proxy-URL env var is set and non-empty in `env`.
#[allow(dead_code)]
pub fn has_proxy_url_env_vars(env: &HashMap<String, String>) -> bool {
    PROXY_URL_ENV_KEYS
        .iter()
        .any(|key| env.get(*key).map(|v| !v.is_empty()).unwrap_or(false))
}

/// Returns the value of the given proxy env key from `env`, if present and non-empty.
#[allow(dead_code)]
pub fn proxy_url_env_value<'a>(env: &'a HashMap<String, String>, key: &str) -> Option<&'a str> {
    env.get(key).map(String::as_str).filter(|v| !v.is_empty())
}

/// Stub network proxy — no MITM-proxy functionality, always acts as "no proxy".
#[derive(Clone, Debug, Default)]
pub struct NetworkProxy;

impl NetworkProxy {
    /// Returns None — no MITM CA bundle in this fork.
    pub fn managed_mitm_ca_trust_bundle_path(_this: &Self) -> Option<&AbsolutePathBuf> {
        None
    }

    /// No-op — does not inject any proxy vars.
    pub fn apply_to_env_for_optional_environment(
        &self,
        _env: &mut HashMap<String, String>,
        _environment_id: Option<&str>,
    ) -> Result<(), String> {
        Ok(())
    }

    /// Always false — no permissive unix socket policy.
    pub fn dangerously_allow_all_unix_sockets(&self) -> bool {
        false
    }

    /// Always empty — no explicit unix socket allowlist.
    pub fn allow_unix_sockets(&self) -> &[String] {
        &[]
    }

    /// Always false — no local binding allowed by default.
    pub fn allow_local_binding(&self) -> bool {
        false
    }
}

/// Decision types that were in the dropped crate (also used by protocol/network_policy.rs,
/// which inlines its own copy — see protocol/src/network_policy.rs).
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkPolicyDecision {
    Allow,
    Deny,
    Ask,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkDecisionSource {
    Decider,
    Policy,
    Default,
}
