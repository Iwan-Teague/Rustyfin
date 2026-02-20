use serde::{Deserialize, Serialize};

/// Setup state machine as defined in the OpenAPI spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetupState {
    NotStarted,
    SessionClaimed,
    ServerConfigSaved,
    AdminCreated,
    LibrariesSaved,
    MetadataSaved,
    NetworkSaved,
    Completed,
}

impl SetupState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NotStarted => "NotStarted",
            Self::SessionClaimed => "SessionClaimed",
            Self::ServerConfigSaved => "ServerConfigSaved",
            Self::AdminCreated => "AdminCreated",
            Self::LibrariesSaved => "LibrariesSaved",
            Self::MetadataSaved => "MetadataSaved",
            Self::NetworkSaved => "NetworkSaved",
            Self::Completed => "Completed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "NotStarted" => Some(Self::NotStarted),
            "SessionClaimed" => Some(Self::SessionClaimed),
            "ServerConfigSaved" => Some(Self::ServerConfigSaved),
            "AdminCreated" => Some(Self::AdminCreated),
            "LibrariesSaved" => Some(Self::LibrariesSaved),
            "MetadataSaved" => Some(Self::MetadataSaved),
            "NetworkSaved" => Some(Self::NetworkSaved),
            "Completed" => Some(Self::Completed),
            _ => None,
        }
    }

    fn ordinal(&self) -> u8 {
        match self {
            Self::NotStarted => 0,
            Self::SessionClaimed => 1,
            Self::ServerConfigSaved => 2,
            Self::AdminCreated => 3,
            Self::LibrariesSaved => 4,
            Self::MetadataSaved => 5,
            Self::NetworkSaved => 6,
            Self::Completed => 7,
        }
    }

    /// Check if the current state is at least the given minimum state.
    pub fn is_at_least(&self, min: SetupState) -> bool {
        self.ordinal() >= min.ordinal()
    }

    /// Check if setup is completed.
    pub fn is_completed(&self) -> bool {
        matches!(self, Self::Completed)
    }
}

impl std::fmt::Display for SetupState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
