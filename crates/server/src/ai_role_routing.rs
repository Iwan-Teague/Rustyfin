use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AiRoleRoutingDecision {
    pub role: String,
    pub model_name: String,
    pub backend_id: String,
    pub backend_kind: String,
    pub selection_source: String,
    pub recommendation_status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_note: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_model_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_updated_ts: Option<i64>,
}

#[cfg(feature = "ai")]
impl From<&crate::ai_model_routing::RoleRoutingDecision> for AiRoleRoutingDecision {
    fn from(value: &crate::ai_model_routing::RoleRoutingDecision) -> Self {
        Self {
            role: value.role.as_str().to_string(),
            model_name: value.model_name.clone(),
            backend_id: value.backend_id.clone(),
            backend_kind: value.backend_kind.as_str().to_string(),
            selection_source: value.selection_source.as_str().to_string(),
            recommendation_status: value.recommendation_status.as_str().to_string(),
            recommendation_note: value.recommendation_note.clone(),
            recommendation_model_name: value.recommendation_model_name.clone(),
            recommendation_updated_ts: value.recommendation_updated_ts,
        }
    }
}
