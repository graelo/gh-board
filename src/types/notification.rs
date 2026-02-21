use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::common::RepoRef;
use super::issue::SubjectType;

// ---------------------------------------------------------------------------
// Notification-specific enums
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationReason {
    Subscribed,
    ReviewRequested,
    Mention,
    Author,
    Comment,
    Assign,
    StateChange,
    CiActivity,
    TeamMention,
    SecurityAlert,
    #[serde(other)]
    Unknown,
}

impl NotificationReason {
    /// Stable display name for filtering and UI rendering.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Subscribed => "subscribed",
            Self::ReviewRequested => "review requested",
            Self::Mention => "mention",
            Self::Author => "author",
            Self::Comment => "comment",
            Self::Assign => "assigned",
            Self::StateChange => "state change",
            Self::CiActivity => "ci activity",
            Self::TeamMention => "team mention",
            Self::SecurityAlert => "security alert",
            Self::Unknown => "other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationStatus {
    Unread,
    Read,
}

// ---------------------------------------------------------------------------
// Notification domain type
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub subject_type: Option<SubjectType>,
    pub subject_title: String,
    pub reason: NotificationReason,
    #[serde(default)]
    pub unread: bool,
    pub repository: Option<RepoRef>,
    #[serde(default)]
    pub url: String,
    pub updated_at: DateTime<Utc>,
}
