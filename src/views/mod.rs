pub mod actions;
pub mod issues;
pub mod notifications;
pub mod prs;
pub mod repo;

use crate::theme::ResolvedTheme;

/// Maximum number of ephemeral tabs per view (session-scoped, created by deep-linking).
pub(crate) const MAX_EPHEMERAL_TABS: usize = 5;

/// Fallback theme when none is provided via props.
pub(crate) fn default_theme() -> ResolvedTheme {
    use crate::config::types::Theme;
    use crate::theme::Background;
    ResolvedTheme::resolve(&Theme::default(), Background::Dark)
}
