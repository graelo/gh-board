use std::borrow::Cow;
use std::sync::LazyLock;

use chrono::{DateTime, Utc};

static EMOJI_REPLACER: LazyLock<gh_emoji::Replacer> = LazyLock::new(gh_emoji::Replacer::new);

/// Expand GitHub emoji shortcodes (e.g. `:tada:` → 🎉) in the given text.
///
/// Returns `Cow::Borrowed` when no shortcodes are found, avoiding allocation.
pub(crate) fn expand_emoji(text: &str) -> Cow<'_, str> {
    EMOJI_REPLACER.replace_all(text)
}

/// Format a datetime according to the configured date format.
///
/// If `date_format` is `"relative"` (or empty/default), displays relative
/// times like `"2h"`, `"3d"`, `"1w"`. Otherwise, uses `strftime`-style
/// formatting.
pub(crate) fn format_date(dt: &DateTime<Utc>, date_format: &str) -> String {
    if date_format.is_empty() || date_format == "relative" {
        format_relative_time(dt)
    } else {
        dt.format(date_format).to_string()
    }
}

/// Format the elapsed duration between two optional timestamps.
///
/// Returns e.g. `"12s"`, `"2m 05s"`, or an empty string when either timestamp
/// is `None`.
pub(crate) fn format_duration(
    started_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
) -> String {
    let (Some(start), Some(end)) = (started_at, completed_at) else {
        return String::new();
    };
    let secs = (end - start).num_seconds().max(0).cast_unsigned();
    if secs < 60 {
        format!("{secs}s")
    } else {
        let m = secs / 60;
        let s = secs % 60;
        format!("{m}m {s:02}s")
    }
}

/// Format a datetime as relative time (e.g., `"2h"`, `"3d"`, `"1w"`).
fn format_relative_time(dt: &DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(dt);

    let minutes = duration.num_minutes();
    if minutes < 1 {
        return "now".to_owned();
    }
    if minutes < 60 {
        return format!("{minutes}m");
    }

    let hours = duration.num_hours();
    if hours < 24 {
        return format!("{hours}h");
    }

    let days = duration.num_days();
    if days < 7 {
        return format!("{days}d");
    }
    if days < 30 {
        return format!("{}w", days / 7);
    }
    if days < 365 {
        return format!("{}mo", days / 30);
    }

    format!("{}y", days / 365)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_shortcode_is_expanded() {
        assert_eq!(expand_emoji(":tada:").as_ref(), "🎉");
    }

    #[test]
    fn unknown_shortcode_is_left_as_is() {
        assert_eq!(
            expand_emoji(":not_a_real_emoji:").as_ref(),
            ":not_a_real_emoji:"
        );
    }

    #[test]
    fn text_without_shortcodes_is_unchanged() {
        let result = expand_emoji("Hello, world!");
        assert!(matches!(result, Cow::Borrowed(_)));
        assert_eq!(result.as_ref(), "Hello, world!");
    }

    #[test]
    fn mixed_text_expands_only_known() {
        assert_eq!(
            expand_emoji("Hello :tada: :unknown:").as_ref(),
            "Hello 🎉 :unknown:"
        );
    }
}
