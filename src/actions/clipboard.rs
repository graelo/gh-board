use std::process::Command;

// ---------------------------------------------------------------------------
// Clipboard operations (T091 — FR-143)
// ---------------------------------------------------------------------------

/// Copy text to the system clipboard.
pub(crate) fn copy_to_clipboard(text: &str) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        Command::new("pbcopy")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
    } else if cfg!(target_os = "linux") {
        // Try xclip first, fall back to xsel.
        Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(stdin) = child.stdin.as_mut() {
                    stdin.write_all(text.as_bytes())?;
                }
                child.wait()
            })
            .or_else(|_| {
                Command::new("xsel")
                    .args(["--clipboard", "--input"])
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .and_then(|mut child| {
                        use std::io::Write;
                        if let Some(stdin) = child.stdin.as_mut() {
                            stdin.write_all(text.as_bytes())?;
                        }
                        child.wait()
                    })
            })
    } else {
        return Err("Clipboard not supported on this platform".to_owned());
    };

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("Clipboard command exited with {status}")),
        Err(e) => Err(format!("Failed to run clipboard command: {e}")),
    }
}

// ---------------------------------------------------------------------------
// Open in browser (T092 — FR-144)
// ---------------------------------------------------------------------------

/// Open a URL in the default browser.
pub(crate) fn open_in_browser(url: &str) -> Result<(), String> {
    let result = if cfg!(target_os = "macos") {
        Command::new("open").arg(url).status()
    } else if cfg!(target_os = "linux") {
        Command::new("xdg-open").arg(url).status()
    } else {
        return Err("Browser open not supported on this platform".to_owned());
    };

    match result {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!("Browser command exited with {status}")),
        Err(e) => Err(format!("Failed to open browser: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_empty_string_succeeds_or_fails_gracefully() {
        // On CI without clipboard tools, this may fail — that's OK.
        let result = copy_to_clipboard("");
        // We just check it doesn't panic.
        let _ = result;
    }
}
