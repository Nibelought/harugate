use std::{io::IsTerminal, path::PathBuf};

use anyhow::Result;
use clap::{Parser, Subcommand};
use openrouter_proxy::{config, hosts, server, state};

/// A local proxy that adds OpenRouter's `provider` routing object to chat requests.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Config file (default: <your config dir>/openrouter-proxy/config.toml, created on
    /// first `serve`).
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Run the proxy and its web UI.
    Serve {
        /// Open the web UI in your default browser once the proxy is up.
        #[arg(long)]
        open: bool,
    },
    /// List the endpoints OpenRouter has for a model, to find a tag to pin.
    Hosts {
        /// OpenRouter model id, `author/slug`.
        model: String,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    openrouter_proxy::init_logging();
    let cli = Cli::parse();
    // No subcommand is `serve`. Started by double-clicking (or typed bare in a terminal),
    // it also opens the web UI, so nobody needs the command line; scripts, services and
    // pipes (stdin or stdout not a terminal) never open a browser, nor does an explicit
    // `serve` without `--open`.
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let command = cli.command.unwrap_or(Command::Serve { open: interactive });
    match command {
        Command::Serve { open } => server::run(cli.config, open).await,
        Command::Hosts { model } => {
            let (path, _) = server::resolve_config_path(cli.config)?;
            // The upstream comes from the config if there is one, else the default.
            let upstream = if path.exists() {
                config::load(&path)?.upstream
            } else {
                config::DEFAULT_UPSTREAM.to_owned()
            };
            let key = std::env::var(state::API_KEY_ENV).ok();
            let list =
                hosts::hosts(&state::build_client()?, &upstream, key.as_deref(), &model).await?;
            if list.is_empty() {
                eprintln!("OpenRouter lists no endpoints with a tag for `{model}`");
            } else {
                print!("{}", hosts::render(&list));
            }
            Ok(())
        }
    }
}
