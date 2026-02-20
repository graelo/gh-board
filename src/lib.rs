// Pedantic: suppress noise for internal crate code.
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::must_use_candidate)]
#![allow(clippy::return_self_not_must_use)]

pub mod actions;
pub mod app;
pub mod color;
pub mod components;
pub mod config;
pub mod engine;
pub mod filter;
pub mod git;
pub(crate) mod github;
pub mod icons;
pub mod init;
pub mod markdown;
pub mod theme;
pub mod types;
pub mod util;
pub mod views;
