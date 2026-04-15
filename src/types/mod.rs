// Shared domain types — used by both the engine layer and the UI layer.
// Neither layer depends on the other; both import from this module.

pub mod common;
pub mod issue;
pub mod notification;
pub mod pr;
pub mod security_alert;
pub mod workflow_run;

pub use common::*;
pub use issue::*;
pub use notification::*;
pub use pr::*;
pub use security_alert::*;
pub use workflow_run::*;
