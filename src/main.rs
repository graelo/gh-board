use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use iocraft::prelude::*;

use gh_board::app::{App, NavigationTarget};
use gh_board::color::ColorDepth;
use gh_board::config::builtin_themes;
use gh_board::config::keybindings::MergedBindings;
use gh_board::config::loader;
use gh_board::engine::{Engine, GitHubEngine};
use gh_board::theme::{Background, ResolvedTheme};
use gh_board::url::{ParsedGitHubUrl, parse_github_url};

#[derive(Parser)]
#[command(name = "gh-board", version, about = "GitHub TUI Dashboard")]
struct Cli {
    /// Path to config file.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Enable debug logging to debug.log.
    #[arg(long)]
    debug: bool,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Shorthand: `gh-board <URL>` (prefer `gh-board open <URL>`).
    #[arg(value_name = "URL")]
    url: Option<String>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new configuration file interactively.
    Init,
    /// List available built-in themes.
    Themes,
    /// Open a GitHub URL directly in the appropriate view.
    Open {
        /// GitHub PR, issue, or actions run URL.
        url: String,
    },
}

/// Convert a parsed GitHub URL into a navigation target.
fn nav_target_from_url(url: &str) -> Result<NavigationTarget> {
    let parsed =
        parse_github_url(url).ok_or_else(|| anyhow::anyhow!("unrecognised GitHub URL: {url}"))?;
    Ok(match parsed {
        ParsedGitHubUrl::PullRequest {
            host,
            owner,
            repo,
            number,
        } => NavigationTarget::PullRequest {
            owner,
            repo,
            number,
            host,
        },
        ParsedGitHubUrl::Issue {
            host,
            owner,
            repo,
            number,
        } => NavigationTarget::Issue {
            owner,
            repo,
            number,
            host,
        },
        ParsedGitHubUrl::ActionsRun {
            host,
            owner,
            repo,
            run_id,
        } => NavigationTarget::ActionsRun {
            owner,
            repo,
            run_id,
            host,
        },
    })
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

    // Handle subcommands that don't need the TUI.
    let open_url: Option<String> = match cli.command {
        Some(Commands::Themes) => {
            for name in builtin_themes::list() {
                println!("{name}");
            }
            return Ok(());
        }
        Some(Commands::Init) => {
            return gh_board::init::run();
        }
        Some(Commands::Open { url }) => Some(url),
        None => {
            if let Some(ref url) = cli.url {
                eprintln!("hint: use \"gh-board open <URL>\" for clarity");
                Some(url.clone())
            } else {
                None
            }
        }
    };

    // Parse the URL into a navigation target (if provided).
    let initial_nav_target: Option<NavigationTarget> =
        open_url.as_deref().map(nav_target_from_url).transpose()?;

    // Set up tracing.
    if cli.debug {
        let file = std::fs::File::create("debug.log")?;
        tracing_subscriber::fmt()
            .with_writer(file)
            .with_ansi(false)
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
    let keybindings = MergedBindings::from_config(&config.keybindings);

    // Install the rustls CryptoProvider before any TLS client is constructed.
    // reqwest 0.13 / rustls 0.23 no longer auto-installs a provider.
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("failed to install default CryptoProvider");

    // Start the GitHub backend engine in a dedicated OS thread (owns its own
    // Tokio runtime). Dropping `engine_handle` at the end of `main` closes the
    // sender channel, signalling the engine to shut down.
    let engine_handle = GitHubEngine::new(config.clone()).start();

    tracing::info!("gh-board starting");

    let cwd = std::env::current_dir().ok();
    let detected_repo = cwd.as_deref().and_then(gh_board::git::detect_repo);

    // Enter fullscreen TUI (iocraft uses smol internally).
    smol::block_on(
        element! {
            App(
                config: &config,
                engine: &engine_handle,
                theme: &theme,
                keybindings: &keybindings,
                color_depth,
                repo_path: cwd.as_deref(),
                detected_repo: detected_repo.as_ref(),
                initial_nav_target,
            )
        }
        .fullscreen(),
    )?;

    Ok(())
}
