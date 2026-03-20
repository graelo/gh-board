// engine module — GitHub backend engine

pub mod github;
mod interface;
mod refresh;
pub mod stub;
pub(crate) mod watch;

pub use github::GitHubEngine;
pub use interface::{Engine, EngineHandle, Event, PrRef, Request};
pub use refresh::{FilterConfig, RefreshScheduler};
pub use stub::StubEngine;
