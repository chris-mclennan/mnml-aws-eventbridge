mod app;
mod clipboard;
mod config;
mod eventbridge;
mod keys;
mod theme;
mod ui;

use anyhow::Result;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(
    name = "mnml-aws-eventbridge",
    version,
    about = "AWS EventBridge buses + rules browser for mnml"
)]
struct Cli {
    /// Print the resolved config + auth state and exit.
    #[arg(long)]
    check: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let cfg = config::load()?;

    if cli.check {
        println!("config: {}", config::config_path().display());
        println!("region: {:?}", cfg.region);
        for (i, t) in cfg.tabs.iter().enumerate() {
            println!(
                "  tab {} ({}): kind={} bus={:?} region={:?}",
                i + 1,
                t.name,
                t.kind,
                t.event_bus_name,
                t.region
            );
        }
        println!("(auth: defers to the `aws` CLI's own credential chain)");
        return Ok(());
    }

    let mut app = app::App::new(cfg)?;
    ui::run(&mut app).await
}
