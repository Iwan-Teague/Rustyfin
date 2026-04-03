use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Planner,
    Summarizer,
    Answer,
    Verifier,
    Worker,
}

impl ModelRole {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planner => "planner",
            Self::Summarizer => "summarizer",
            Self::Answer => "answer",
            Self::Verifier => "verifier",
            Self::Worker => "worker",
        }
    }

    pub const fn all() -> [Self; 5] {
        [
            Self::Planner,
            Self::Summarizer,
            Self::Answer,
            Self::Verifier,
            Self::Worker,
        ]
    }
}
