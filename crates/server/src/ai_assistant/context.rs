use crate::auth::AuthUser;

#[derive(Debug, Clone)]
pub struct AssistantContext {
    pub trace_id: String,
    pub user_id: String,
    pub username: String,
    pub role: String,
    pub is_admin: bool,
    pub confirmed_write_tool: Option<String>,
    pub conversation_id: Option<String>,
}

impl AssistantContext {
    pub fn new(user: &AuthUser, trace_id: impl Into<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            user_id: user.user_id.clone(),
            username: user.username.clone(),
            role: user.role.clone(),
            is_admin: user.role == "admin",
            confirmed_write_tool: None,
            conversation_id: None,
        }
    }

    pub fn with_confirmed_write_tool(mut self, tool_name: &str) -> Self {
        self.confirmed_write_tool = Some(tool_name.to_string());
        self
    }

    pub fn with_conversation_id(mut self, conversation_id: Option<&str>) -> Self {
        self.conversation_id = conversation_id.map(str::to_string);
        self
    }
}
