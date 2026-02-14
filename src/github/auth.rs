use std::process::Command;

use anyhow::{Context, Result, bail};

/// Resolve a GitHub auth token for the given host.
///
/// Priority:
/// 1. `gh auth token --hostname {host}` (gh CLI)
/// 2. `GH_TOKEN` environment variable
/// 3. `GITHUB_TOKEN` environment variable
pub fn resolve_token(host: &str) -> Result<String> {
    // Try gh CLI first.
    if let Ok(token) = token_from_gh_cli(host) {
        return Ok(token);
    }

    // Fall back to environment variables.
    if let Ok(token) = std::env::var("GH_TOKEN")
        && !token.is_empty()
    {
        return Ok(token);
    }
    if let Ok(token) = std::env::var("GITHUB_TOKEN")
        && !token.is_empty()
    {
        return Ok(token);
    }

    bail!(
        "no GitHub token found for host \"{host}\". \
         Run `gh auth login` or set GH_TOKEN / GITHUB_TOKEN."
    )
}

fn token_from_gh_cli(host: &str) -> Result<String> {
    let output = Command::new("gh")
        .args(["auth", "token", "--hostname", host])
        .output()
        .context("failed to run `gh auth token`")?;

    if !output.status.success() {
        bail!("gh auth token exited with non-zero status");
    }

    let token = String::from_utf8(output.stdout)
        .context("gh auth token produced non-UTF-8 output")?
        .trim()
        .to_owned();

    if token.is_empty() {
        bail!("gh auth token returned empty string");
    }

    Ok(token)
}
