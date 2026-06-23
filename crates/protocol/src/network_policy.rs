use crate::approvals::NetworkApprovalProtocol;
use serde::Deserialize;
use serde::Serialize;

/// Whether the network proxy allowed or denied a request (inlined from codex_network_proxy).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NetworkPolicyDecision {
    Allow,
    Deny,
    Ask,
}

/// Source that produced the network policy decision (inlined from codex_network_proxy).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum NetworkDecisionSource {
    Decider,
    Policy,
    Default,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkPolicyDecisionPayload {
    pub decision: NetworkPolicyDecision,
    pub source: NetworkDecisionSource,
    #[serde(default)]
    pub protocol: Option<NetworkApprovalProtocol>,
    pub host: Option<String>,
    pub reason: Option<String>,
    pub port: Option<u16>,
}

impl NetworkPolicyDecisionPayload {
    pub fn is_ask_from_decider(&self) -> bool {
        self.decision == NetworkPolicyDecision::Ask && self.source == NetworkDecisionSource::Decider
    }
}
