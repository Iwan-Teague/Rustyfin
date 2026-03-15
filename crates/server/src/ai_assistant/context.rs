use crate::auth::AuthUser;

#[derive(Debug, Clone)]
pub struct AssistantContext {
    pub trace_id: String,
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub is_admin: bool,
}

impl AssistantContext {
    pub fn new(user: &AuthUser, trace_id: impl Into<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            user_id: user.user_id.clone(),
            username: user.username.clone(),
            role: user.role.clone(),
            is_admin: user.role == "admin",
        }
    }
}
