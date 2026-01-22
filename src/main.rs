mod core;
mod plugins;

use clap::{Parser, Subcommand};
use core::engine::Engine;
use core::events::EngineEvent;
use core::model::LinkInput;
use plugins::registry::PluginRegistry;
use plugins::registry::DriverContext;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "downloader")]
#[command(about = "Multi-fragment downloader (HTTP + GitHub resolver) - plugin based", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Download one or more links
    Download {
        /// Links to download
        links: Vec<String>,

        /// Output directory
        #[arg(long, default_value = "./downloads")]
        out_dir: PathBuf,

        /// Max concurrent fragments per item
        #[arg(long, default_value_t = 6)]
        concurrency: usize,

        /// Chunk size in MB (for HTTP range)
        #[arg(long, default_value_t = 8)]
        chunk_mb: u64,

        /// Extra HTTP header (repeatable), e.g. --header 'Authorization: Bearer xxx'
        #[arg(long = "header")]
        headers: Vec<String>,

        /// HTTP User-Agent
        #[arg(long, default_value = "OrangeDownloader/0.1")]
        user_agent: String,

        /// HTTP timeout in seconds
        #[arg(long, default_value_t = 60)]
        timeout_secs: u64,

        /// HTTP retries for transient errors
        #[arg(long, default_value_t = 2)]
        retries: u32,

        /// Retry backoff base in milliseconds
        #[arg(long, default_value_t = 400)]
        retry_backoff_ms: u64,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Download {
            links,
            out_dir,
            concurrency,
            chunk_mb,
            headers,
            user_agent,
            timeout_secs,
            retries,
            retry_backoff_ms,
        } => {
            if links.is_empty() {
                anyhow::bail!("no links provided");
            }

            tokio::fs::create_dir_all(&out_dir).await?;

            let registry = PluginRegistry::with_defaults();
            let driver_ctx = DriverContext {
                user_agent,
                timeout_secs,
                retries,
                retry_backoff_ms,
            };
            let engine = Engine::new(registry, out_dir.clone(), concurrency, chunk_mb * 1024 * 1024, driver_ctx).await?;

            let mut rx = engine.subscribe();
            tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(evt) => print_event(evt),
                        Err(_) => break,
                    }
                }
            });

            let mut header_map = HashMap::new();
            for h in headers {
                let (k, v) = h
                    .split_once(':')
                    .ok_or_else(|| anyhow::anyhow!("invalid header format: {}", h))?;
                header_map.insert(k.trim().to_string(), v.trim().to_string());
            }

            let inputs: Vec<LinkInput> = links
                .into_iter()
                .map(|raw| LinkInput {
                    raw,
                    headers: header_map.clone(),
                    options: HashMap::new(),
                })
                .collect();

            let job_id = engine.add_and_start(inputs).await?;
            println!("Job started: {}", job_id);

            engine.wait_job(job_id).await;

            println!("Job finished: {}", job_id);
        }
    }

    Ok(())
}

fn print_event(evt: EngineEvent) {
    match evt {
        EngineEvent::JobStatusChanged { job_id, status } => {
            println!("[JOB] {} -> {:?}", job_id, status);
        }
        EngineEvent::ItemAdded { item_id, display_name, target_path, uri } => {
            println!("  [ITEM] {} added: {} -> {} ({})", item_id, display_name, target_path.display(), uri);
        }
        EngineEvent::ItemStatusChanged { item_id, status } => {
            println!("  [ITEM] {} -> {:?}", item_id, status);
        }
        EngineEvent::Progress {
            item_id,
            downloaded,
            total,
            speed_bps,
            eta,
        } => {
            let total_s = total.map(|t| format!("{}", t)).unwrap_or_else(|| "?".into());
            let eta_s = eta.map(|d| format!("{:.1}s", d.as_secs_f64())).unwrap_or_else(|| "-".into());
            println!(
                "    [PROG] item={} {} / {} bytes | {:.2} MB/s | eta {}",
                item_id,
                downloaded,
                total_s,
                (speed_bps as f64) / (1024.0 * 1024.0),
                eta_s
            );
        }
        EngineEvent::FragmentDone { item_id, completed, total } => {
            println!("    [FRAG] item={} fragments {}/{}", item_id, completed, total);
        }
        EngineEvent::Error { scope, message } => {
            eprintln!("  [ERR] {}: {}", scope, message);
        }
        EngineEvent::Info { scope, message } => {
            println!("  [INFO] {}: {}", scope, message);
        }

    }
}
