use std::fmt;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// SecurityAlert enums
// ---------------------------------------------------------------------------

/// The three categories of security alerts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertCategory {
    Dependabot,
    CodeScanning,
    SecretScanning,
}

impl fmt::Display for AlertCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dependabot => f.write_str("dependabot"),
            Self::CodeScanning => f.write_str("code scan"),
            Self::SecretScanning => f.write_str("secret"),
        }
    }
}

/// Unified severity levels across all three alert types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertSeverity {
    Critical,
    High,
    Medium,
    Low,
    #[serde(other)]
    Unknown,
}

impl fmt::Display for AlertSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Critical => f.write_str("critical"),
            Self::High => f.write_str("high"),
            Self::Medium => f.write_str("medium"),
            Self::Low => f.write_str("low"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

/// Unified alert state across all three API state sets.
///
/// `Fixed` maps to dependabot "fixed" and code scanning "fixed".
/// `AutoDismissed` is dependabot-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertState {
    Open,
    Dismissed,
    Fixed,
    AutoDismissed,
    #[serde(other)]
    Unknown,
}

impl fmt::Display for AlertState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Open => f.write_str("open"),
            Self::Dismissed => f.write_str("dismissed"),
            Self::Fixed => f.write_str("fixed"),
            Self::AutoDismissed => f.write_str("auto-dismissed"),
            Self::Unknown => f.write_str("unknown"),
        }
    }
}

// ---------------------------------------------------------------------------
// SecurityAlert domain type
// ---------------------------------------------------------------------------

/// Unified domain type for table display — one row in the alerts table.
///
/// `package_or_rule` is: package name (Dependabot), rule ID (Code Scanning),
/// or secret type (Secrets).
/// `summary` is: advisory summary / rule description /
/// `secret_type_display_name`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityAlert {
    pub number: u64,
    pub category: AlertCategory,
    pub severity: AlertSeverity,
    pub state: AlertState,
    pub package_or_rule: String,
    pub summary: String,
    /// `"owner/repo"` slug.
    pub repo: String,
    pub html_url: String,
    pub created_at: DateTime<Utc>,
    pub detail: AlertDetail,
}

// ---------------------------------------------------------------------------
// AlertDetail — category-specific data for the sidebar
// ---------------------------------------------------------------------------

/// Typed detail preserved from the API, used by the sidebar for rendering
/// category-specific information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertDetail {
    Dependabot {
        ecosystem: String,
        ghsa_id: String,
        cve_id: Option<String>,
        vulnerable_version_range: Option<String>,
        patched_version: Option<String>,
    },
    CodeScanning {
        tool_name: String,
        tool_version: Option<String>,
        rule_id: String,
        rule_description: String,
        instances: Vec<CodeScanningInstance>,
    },
    SecretScanning {
        secret_type: String,
        secret_type_display_name: String,
        validity: Option<String>,
        resolution: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// Supporting structs
// ---------------------------------------------------------------------------

/// A single code scanning instance (location where the finding was detected).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeScanningInstance {
    pub ref_name: Option<String>,
    pub path: Option<String>,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
    pub state: AlertState,
}

/// A single secret scanning location.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretLocation {
    /// `"commit"`, `"wiki_commit"`, `"issue_title"`, etc.
    pub location_type: String,
    pub path: Option<String>,
    pub start_line: Option<u64>,
    pub end_line: Option<u64>,
}
