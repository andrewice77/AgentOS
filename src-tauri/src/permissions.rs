use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    Safe,
    Read,
    Operate,
    Admin,
}

impl PermissionLevel {
    pub fn from_str_cfg(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "safe" => Self::Safe,
            "read" => Self::Read,
            "admin" => Self::Admin,
            _ => Self::Operate,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Safe => "safe",
            Self::Read => "read",
            Self::Operate => "operate",
            Self::Admin => "admin",
        }
    }

    pub fn requires_confirmation(self) -> bool {
        matches!(self, Self::Operate | Self::Admin)
    }
}

pub fn level_for_mcp(default_mcp: &str) -> PermissionLevel {
    PermissionLevel::from_str_cfg(default_mcp)
}
