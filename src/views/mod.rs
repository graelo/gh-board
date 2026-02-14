pub mod issues;
pub mod notifications;
pub mod prs;
pub mod repo;

use crate::theme::ResolvedTheme;

/// Fallback theme when none is provided via props.
pub(crate) fn default_theme() -> ResolvedTheme {
    use crate::config::types::Theme;
    use crate::theme::Background;
    ResolvedTheme::resolve(&Theme::default(), Background::Dark)
}
