use std::sync::Arc;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use octocrab::Octocrab;
use serde::Deserialize;

use crate::github::client::extract_rest_rate_limit;
use crate::types::{
    AlertCategory, AlertDetail, AlertSeverity, AlertState, CodeScanningInstance, RateLimitInfo,
    SecretLocation, SecurityAlert,
};

// ── Raw API response types: Dependabot ───────────────────────────────

#[derive(Deserialize)]
struct RawDependabotAlert {
    number: u64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    dependency: RawDependency,
    #[serde(default)]
    security_advisory: Option<RawSecurityAdvisory>,
    #[serde(default)]
    security_vulnerability: Option<RawSecurityVulnerability>,
    #[serde(default)]
    html_url: String,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize, Default)]
struct RawDependency {
    #[serde(default)]
    package: Option<RawPackage>,
}

#[derive(Deserialize)]
struct RawPackage {
    #[serde(default)]
    ecosystem: String,
    #[serde(default)]
    name: String,
}

#[derive(Deserialize)]
struct RawSecurityAdvisory {
    #[serde(default)]
    ghsa_id: String,
    #[serde(default)]
    cve_id: Option<String>,
    #[serde(default)]
    summary: String,
    #[serde(default)]
    severity: Option<String>,
}

#[derive(Deserialize)]
struct RawSecurityVulnerability {
    #[serde(default)]
    vulnerable_version_range: Option<String>,
    #[serde(default)]
    first_patched_version: Option<RawPatchedVersion>,
}

#[derive(Deserialize)]
struct RawPatchedVersion {
    #[serde(default)]
    identifier: Option<String>,
}

// ── Raw API response types: Code Scanning ────────────────────────────

#[derive(Deserialize)]
struct RawCodeScanningAlert {
    number: u64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    rule: RawRule,
    #[serde(default)]
    tool: RawTool,
    #[serde(default)]
    most_recent_instance: Option<RawCodeScanningInstance>,
    #[serde(default)]
    html_url: String,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize, Default)]
struct RawRule {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    severity: Option<String>,
    #[serde(default)]
    security_severity_level: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawTool {
    #[serde(default)]
    name: String,
    #[serde(default)]
    version: Option<String>,
}

#[derive(Deserialize)]
struct RawCodeScanningInstance {
    #[serde(default, rename = "ref")]
    ref_name: Option<String>,
    #[serde(default)]
    location: Option<RawLocation>,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
struct RawLocation {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    start_line: Option<u64>,
    #[serde(default)]
    end_line: Option<u64>,
}

// ── Raw API response types: Secret Scanning ──────────────────────────

#[derive(Deserialize)]
struct RawSecretScanningAlert {
    number: u64,
    #[serde(default)]
    state: String,
    #[serde(default)]
    secret_type: String,
    #[serde(default)]
    secret_type_display_name: String,
    #[serde(default)]
    validity: Option<String>,
    #[serde(default)]
    resolution: Option<String>,
    #[serde(default)]
    html_url: String,
    created_at: DateTime<Utc>,
}

#[derive(Deserialize)]
struct RawSecretLocation {
    #[serde(rename = "type")]
    location_type: String,
    #[serde(default)]
    details: Option<RawSecretLocationDetails>,
}

#[derive(Deserialize)]
struct RawSecretLocationDetails {
    #[serde(default)]
    path: Option<String>,
    #[serde(default)]
    start_line: Option<u64>,
    #[serde(default)]
    end_line: Option<u64>,
}

// ── Conversion helpers ───────────────────────────────────────────────

fn parse_severity(s: Option<&str>) -> AlertSeverity {
    match s {
        Some("critical") => AlertSeverity::Critical,
        Some("high") => AlertSeverity::High,
        Some("medium") => AlertSeverity::Medium,
        Some("low") => AlertSeverity::Low,
        _ => AlertSeverity::Unknown,
    }
}

fn parse_state(s: &str) -> AlertState {
    match s {
        "open" => AlertState::Open,
        "dismissed" | "resolved" => AlertState::Dismissed,
        "fixed" => AlertState::Fixed,
        "auto_dismissed" => AlertState::AutoDismissed,
        _ => AlertState::Unknown,
    }
}

fn dependabot_into_domain(raw: RawDependabotAlert, owner: &str, repo: &str) -> SecurityAlert {
    let advisory = raw.security_advisory.as_ref();
    let vuln = raw.security_vulnerability.as_ref();
    let pkg = raw.dependency.package.as_ref();

    let package_name = pkg.map_or_else(String::new, |p| p.name.clone());

    SecurityAlert {
        number: raw.number,
        category: AlertCategory::Dependabot,
        severity: parse_severity(advisory.and_then(|a| a.severity.as_deref())),
        state: parse_state(&raw.state),
        package_or_rule: package_name,
        summary: advisory.map_or_else(String::new, |a| a.summary.clone()),
        repo: format!("{owner}/{repo}"),
        html_url: raw.html_url,
        created_at: raw.created_at,
        detail: AlertDetail::Dependabot {
            ecosystem: pkg.map_or_else(String::new, |p| p.ecosystem.clone()),
            ghsa_id: advisory.map_or_else(String::new, |a| a.ghsa_id.clone()),
            cve_id: advisory.and_then(|a| a.cve_id.clone()),
            vulnerable_version_range: vuln.and_then(|v| v.vulnerable_version_range.clone()),
            patched_version: vuln.and_then(|v| {
                v.first_patched_version
                    .as_ref()
                    .and_then(|p| p.identifier.clone())
            }),
        },
    }
}

fn code_scanning_into_domain(raw: RawCodeScanningAlert, owner: &str, repo: &str) -> SecurityAlert {
    // Prefer security_severity_level, fall back to severity, then Unknown.
    let severity = parse_severity(
        raw.rule
            .security_severity_level
            .as_deref()
            .or(raw.rule.severity.as_deref()),
    );

    let instances: Vec<CodeScanningInstance> = raw
        .most_recent_instance
        .into_iter()
        .map(|inst| {
            let loc = inst.location.as_ref();
            CodeScanningInstance {
                ref_name: inst.ref_name,
                path: loc.and_then(|l| l.path.clone()),
                start_line: loc.and_then(|l| l.start_line),
                end_line: loc.and_then(|l| l.end_line),
                state: inst
                    .state
                    .as_deref()
                    .map_or(AlertState::Unknown, parse_state),
            }
        })
        .collect();

    SecurityAlert {
        number: raw.number,
        category: AlertCategory::CodeScanning,
        severity,
        state: parse_state(&raw.state),
        package_or_rule: raw.rule.id.clone(),
        summary: raw.rule.name.clone(),
        repo: format!("{owner}/{repo}"),
        html_url: raw.html_url,
        created_at: raw.created_at,
        detail: AlertDetail::CodeScanning {
            tool_name: raw.tool.name,
            tool_version: raw.tool.version,
            rule_id: raw.rule.id,
            rule_description: raw.rule.name,
            instances,
        },
    }
}

fn secret_scanning_into_domain(
    raw: RawSecretScanningAlert,
    owner: &str,
    repo: &str,
) -> SecurityAlert {
    SecurityAlert {
        number: raw.number,
        category: AlertCategory::SecretScanning,
        severity: AlertSeverity::High,
        state: parse_state(&raw.state),
        package_or_rule: raw.secret_type.clone(),
        summary: raw.secret_type_display_name.clone(),
        repo: format!("{owner}/{repo}"),
        html_url: raw.html_url,
        created_at: raw.created_at,
        detail: AlertDetail::SecretScanning {
            secret_type: raw.secret_type,
            secret_type_display_name: raw.secret_type_display_name,
            validity: raw.validity,
            resolution: raw.resolution,
        },
    }
}

// ── Public API ───────────────────────────────────────────────────────

/// Fetch open Dependabot alerts for a repository.
pub async fn fetch_dependabot_alerts(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    limit: u32,
) -> Result<(Vec<SecurityAlert>, Option<RateLimitInfo>)> {
    let per_page = limit.min(100);
    let url = format!(
        "/repos/{owner}/{repo}/dependabot/alerts?per_page={per_page}&state=open&sort=created&direction=desc"
    );
    let response = octocrab
        ._get(url)
        .await
        .context("fetching dependabot alerts")?;

    let rate_limit = extract_rest_rate_limit(response.headers());
    let body = octocrab
        .body_to_string(response)
        .await
        .context("reading dependabot alerts body")?;
    let raw_alerts: Vec<RawDependabotAlert> =
        serde_json::from_str(&body).context("deserializing dependabot alerts")?;

    let alerts: Vec<SecurityAlert> = raw_alerts
        .into_iter()
        .map(|r| dependabot_into_domain(r, owner, repo))
        .collect();

    tracing::debug!(
        "fetched {} dependabot alerts for {}/{}",
        alerts.len(),
        owner,
        repo
    );
    Ok((alerts, rate_limit))
}

/// Fetch open code scanning alerts for a repository.
pub async fn fetch_code_scanning_alerts(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    limit: u32,
) -> Result<(Vec<SecurityAlert>, Option<RateLimitInfo>)> {
    let per_page = limit.min(100);
    let url = format!(
        "/repos/{owner}/{repo}/code-scanning/alerts?per_page={per_page}&state=open&sort=created&direction=desc"
    );
    let response = octocrab
        ._get(url)
        .await
        .context("fetching code scanning alerts")?;

    let rate_limit = extract_rest_rate_limit(response.headers());
    let body = octocrab
        .body_to_string(response)
        .await
        .context("reading code scanning alerts body")?;
    let raw_alerts: Vec<RawCodeScanningAlert> =
        serde_json::from_str(&body).context("deserializing code scanning alerts")?;

    let alerts: Vec<SecurityAlert> = raw_alerts
        .into_iter()
        .map(|r| code_scanning_into_domain(r, owner, repo))
        .collect();

    tracing::debug!(
        "fetched {} code scanning alerts for {}/{}",
        alerts.len(),
        owner,
        repo
    );
    Ok((alerts, rate_limit))
}

/// Fetch open secret scanning alerts for a repository.
pub async fn fetch_secret_scanning_alerts(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    limit: u32,
) -> Result<(Vec<SecurityAlert>, Option<RateLimitInfo>)> {
    let per_page = limit.min(100);
    let url = format!(
        "/repos/{owner}/{repo}/secret-scanning/alerts?per_page={per_page}&state=open&sort=created&direction=desc"
    );
    let response = octocrab
        ._get(url)
        .await
        .context("fetching secret scanning alerts")?;

    let rate_limit = extract_rest_rate_limit(response.headers());
    let body = octocrab
        .body_to_string(response)
        .await
        .context("reading secret scanning alerts body")?;
    let raw_alerts: Vec<RawSecretScanningAlert> =
        serde_json::from_str(&body).context("deserializing secret scanning alerts")?;

    let alerts: Vec<SecurityAlert> = raw_alerts
        .into_iter()
        .map(|r| secret_scanning_into_domain(r, owner, repo))
        .collect();

    tracing::debug!(
        "fetched {} secret scanning alerts for {}/{}",
        alerts.len(),
        owner,
        repo
    );
    Ok((alerts, rate_limit))
}

/// Fetch locations for a specific secret scanning alert.
pub async fn fetch_secret_alert_locations(
    octocrab: &Arc<Octocrab>,
    owner: &str,
    repo: &str,
    alert_number: u64,
) -> Result<(Vec<SecretLocation>, Option<RateLimitInfo>)> {
    let url = format!("/repos/{owner}/{repo}/secret-scanning/alerts/{alert_number}/locations");
    let response = octocrab
        ._get(url)
        .await
        .context("fetching secret alert locations")?;

    let rate_limit = extract_rest_rate_limit(response.headers());
    let body = octocrab
        .body_to_string(response)
        .await
        .context("reading secret alert locations body")?;
    let raw_locations: Vec<RawSecretLocation> =
        serde_json::from_str(&body).context("deserializing secret alert locations")?;

    let locations: Vec<SecretLocation> = raw_locations
        .into_iter()
        .map(|r| {
            let details = r.details.as_ref();
            SecretLocation {
                location_type: r.location_type,
                path: details.and_then(|d| d.path.clone()),
                start_line: details.and_then(|d| d.start_line),
                end_line: details.and_then(|d| d.end_line),
            }
        })
        .collect();

    tracing::debug!(
        "fetched {} locations for secret alert #{} in {}/{}",
        locations.len(),
        alert_number,
        owner,
        repo
    );
    Ok((locations, rate_limit))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_severity_known_values() {
        assert_eq!(parse_severity(Some("critical")), AlertSeverity::Critical);
        assert_eq!(parse_severity(Some("high")), AlertSeverity::High);
        assert_eq!(parse_severity(Some("medium")), AlertSeverity::Medium);
        assert_eq!(parse_severity(Some("low")), AlertSeverity::Low);
    }

    #[test]
    fn parse_severity_unknown_and_none() {
        assert_eq!(parse_severity(Some("bogus")), AlertSeverity::Unknown);
        assert_eq!(parse_severity(None), AlertSeverity::Unknown);
    }

    #[test]
    fn parse_state_known_values() {
        assert_eq!(parse_state("open"), AlertState::Open);
        assert_eq!(parse_state("dismissed"), AlertState::Dismissed);
        assert_eq!(parse_state("resolved"), AlertState::Dismissed); // secret scanning alias
        assert_eq!(parse_state("fixed"), AlertState::Fixed);
        assert_eq!(parse_state("auto_dismissed"), AlertState::AutoDismissed);
    }

    #[test]
    fn parse_state_unknown() {
        assert_eq!(parse_state("whatever"), AlertState::Unknown);
    }

    #[test]
    fn dependabot_into_domain_maps_fields() {
        let json = r#"{
            "number": 42,
            "state": "open",
            "dependency": {
                "package": { "ecosystem": "npm", "name": "lodash" }
            },
            "security_advisory": {
                "ghsa_id": "GHSA-1234-5678-abcd",
                "cve_id": "CVE-2023-12345",
                "summary": "Prototype pollution in lodash",
                "severity": "critical"
            },
            "security_vulnerability": {
                "vulnerable_version_range": "< 4.17.21",
                "first_patched_version": { "identifier": "4.17.21" }
            },
            "html_url": "https://github.com/owner/repo/security/dependabot/42",
            "created_at": "2024-01-15T10:30:00Z"
        }"#;
        let raw: RawDependabotAlert = serde_json::from_str(json).unwrap();
        let alert = dependabot_into_domain(raw, "owner", "repo");

        assert_eq!(alert.number, 42);
        assert_eq!(alert.category, AlertCategory::Dependabot);
        assert_eq!(alert.severity, AlertSeverity::Critical);
        assert_eq!(alert.state, AlertState::Open);
        assert_eq!(alert.package_or_rule, "lodash");
        assert_eq!(alert.summary, "Prototype pollution in lodash");
        assert_eq!(alert.repo, "owner/repo");

        match &alert.detail {
            AlertDetail::Dependabot {
                ecosystem,
                ghsa_id,
                cve_id,
                patched_version,
                ..
            } => {
                assert_eq!(ecosystem, "npm");
                assert_eq!(ghsa_id, "GHSA-1234-5678-abcd");
                assert_eq!(cve_id.as_deref(), Some("CVE-2023-12345"));
                assert_eq!(patched_version.as_deref(), Some("4.17.21"));
            }
            _ => panic!("expected Dependabot detail"),
        }
    }

    #[test]
    fn code_scanning_into_domain_maps_fields() {
        let json = r#"{
            "number": 7,
            "state": "open",
            "rule": {
                "id": "js/sql-injection",
                "name": "SQL injection",
                "severity": "warning",
                "security_severity_level": "high"
            },
            "tool": { "name": "CodeQL", "version": "2.15.0" },
            "most_recent_instance": {
                "ref": "refs/heads/main",
                "state": "open",
                "location": { "path": "src/db.js", "start_line": 42, "end_line": 42 }
            },
            "html_url": "https://github.com/owner/repo/security/code-scanning/7",
            "created_at": "2024-02-20T15:00:00Z",
            "instances_url": "https://api.github.com/repos/owner/repo/code-scanning/alerts/7/instances"
        }"#;
        let raw: RawCodeScanningAlert = serde_json::from_str(json).unwrap();
        let alert = code_scanning_into_domain(raw, "owner", "repo");

        assert_eq!(alert.number, 7);
        assert_eq!(alert.category, AlertCategory::CodeScanning);
        assert_eq!(alert.severity, AlertSeverity::High); // security_severity_level preferred
        assert_eq!(alert.state, AlertState::Open);
        assert_eq!(alert.package_or_rule, "js/sql-injection");
        assert_eq!(alert.summary, "SQL injection");

        match &alert.detail {
            AlertDetail::CodeScanning {
                tool_name,
                tool_version,
                instances,
                ..
            } => {
                assert_eq!(tool_name, "CodeQL");
                assert_eq!(tool_version.as_deref(), Some("2.15.0"));
                assert_eq!(instances.len(), 1);
                assert_eq!(instances[0].path.as_deref(), Some("src/db.js"));
                assert_eq!(instances[0].start_line, Some(42));
            }
            _ => panic!("expected CodeScanning detail"),
        }
    }

    #[test]
    fn secret_scanning_into_domain_maps_fields() {
        let json = r#"{
            "number": 3,
            "state": "open",
            "secret_type": "github_personal_access_token",
            "secret_type_display_name": "GitHub Personal Access Token",
            "validity": "active",
            "resolution": null,
            "html_url": "https://github.com/owner/repo/security/secret-scanning/3",
            "created_at": "2024-03-10T08:00:00Z"
        }"#;
        let raw: RawSecretScanningAlert = serde_json::from_str(json).unwrap();
        let alert = secret_scanning_into_domain(raw, "owner", "repo");

        assert_eq!(alert.number, 3);
        assert_eq!(alert.category, AlertCategory::SecretScanning);
        assert_eq!(alert.severity, AlertSeverity::High); // hardcoded
        assert_eq!(alert.state, AlertState::Open);
        assert_eq!(alert.package_or_rule, "github_personal_access_token");
        assert_eq!(alert.summary, "GitHub Personal Access Token");

        match &alert.detail {
            AlertDetail::SecretScanning {
                secret_type,
                validity,
                ..
            } => {
                assert_eq!(secret_type, "github_personal_access_token");
                assert_eq!(validity.as_deref(), Some("active"));
            }
            _ => panic!("expected SecretScanning detail"),
        }
    }

    #[test]
    fn secret_scanning_resolved_maps_to_dismissed() {
        let json = r#"{
            "number": 5,
            "state": "resolved",
            "secret_type": "aws_access_key",
            "secret_type_display_name": "AWS Access Key",
            "validity": "inactive",
            "resolution": "revoked",
            "html_url": "https://github.com/owner/repo/security/secret-scanning/5",
            "created_at": "2024-03-10T08:00:00Z"
        }"#;
        let raw: RawSecretScanningAlert = serde_json::from_str(json).unwrap();
        let alert = secret_scanning_into_domain(raw, "owner", "repo");
        assert_eq!(alert.state, AlertState::Dismissed);
    }

    #[test]
    fn code_scanning_severity_falls_back_to_rule_severity() {
        let json = r#"{
            "number": 1,
            "state": "open",
            "rule": {
                "id": "some-rule",
                "name": "Some Rule",
                "severity": "medium",
                "security_severity_level": null
            },
            "tool": { "name": "zizmor" },
            "html_url": "https://example.com/1",
            "created_at": "2024-01-01T00:00:00Z",
            "instances_url": ""
        }"#;
        let raw: RawCodeScanningAlert = serde_json::from_str(json).unwrap();
        let alert = code_scanning_into_domain(raw, "o", "r");
        assert_eq!(alert.severity, AlertSeverity::Medium);
    }

    #[test]
    fn dependabot_minimal_json_deserializes() {
        let json = r#"{
            "number": 1,
            "html_url": "",
            "created_at": "2024-01-01T00:00:00Z"
        }"#;
        let raw: RawDependabotAlert = serde_json::from_str(json).unwrap();
        let alert = dependabot_into_domain(raw, "o", "r");
        assert_eq!(alert.number, 1);
        assert_eq!(alert.severity, AlertSeverity::Unknown);
        assert_eq!(alert.package_or_rule, ""); // no package
    }
}
