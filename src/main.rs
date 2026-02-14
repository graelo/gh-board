use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use iocraft::prelude::*;

use gh_board::app::App;
use gh_board::color::ColorDepth;
use gh_board::config::loader;
use gh_board::github::client::GitHubClient;
use gh_board::theme::{Background, ResolvedTheme};

#[derive(Parser)]
#[command(name = "gh-board", version, about = "GitHub TUI Dashboard")]
struct Cli {
    /// Path to config file.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Enable debug logging to debug.log.
    #[arg(long)]
    debug: bool,
}

fn main() -> Result<()> {
    // Install a panic hook that writes to a file, since the fullscreen TUI
    // swallows stderr.
    std::panic::set_hook(Box::new(|info| {
        let backtrace = std::backtrace::Backtrace::force_capture();
        let msg = format!("{info}\n\n{backtrace}");
        let _ = std::fs::write("panic.log", &msg);
        eprintln!("{msg}");
    }));

    let cli = Cli::parse();

    // Set up tracing.
    if cli.debug {
        let file = std::fs::File::create("debug.log")?;
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_env("RUST_LOG")
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug")),
            )
            .init();
    }

    // Load config.
    let config = loader::load_config(cli.config.as_deref())?;

    // Detect terminal capabilities.
    let color_depth = ColorDepth::detect();
    let background = Background::detect();
    let theme = ResolvedTheme::resolve(&config.theme, background);

    // Build a Tokio runtime. Octocrab (via tower's BufferLayer) requires a
    // Tokio reactor to be active when constructing its HTTP service, so we
    // enter the runtime context before creating the client.
    let tokio_rt = tokio::runtime::Runtime::new()?;
    let _guard = tokio_rt.enter();

    // Create GitHub client and get the default host octocrab instance.
    let mut gh_client = GitHubClient::new(config.defaults.refetch_interval_minutes);
    let default_host = "github.com";
    let octocrab = gh_client.octocrab_for(default_host)?;

    tracing::info!("gh-board starting");

    let cwd = std::env::current_dir().ok();

    // Enter fullscreen TUI. iocraft uses smol internally; the Tokio runtime
    // context remains active so that octocrab/tower calls (wrapped in
    // async-compat's `Compat`) can reach the reactor.
    smol::block_on(
        element! {
            App(
                config: &config,
                octocrab: &octocrab,
                theme: &theme,
                color_depth,
                repo_path: cwd.as_deref(),
            )
        }
        .fullscreen(),
    )?;

    Ok(())
}
