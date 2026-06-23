use serde::Deserialize;
use serde::Serialize;

/// Request body for registering an executor with the agentx gateway.
///
/// POST /agentx/environment/{env_id}/register
/// Authorization: AgentAssertion <jwt>
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnvironmentRegistryRegistrationRequest {
    pub id: String,
    pub public_key: String,
}

/// Response from the agentx gateway after executor registration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EnvironmentRegistryRegistrationResponse {
    pub executor_id: String,
    pub url: String,
}
