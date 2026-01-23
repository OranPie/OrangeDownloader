mod core;
mod plugins;

use clap::{Arg, ArgAction, Command};
use core::engine::Engine;
use core::events::EngineEvent;
use core::model::LinkInput;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use plugins::registry::PluginRegistry;
use plugins::registry::DriverContext;
use plugins::registry::DownloadCliConfig;
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

fn build_cli(registry: &PluginRegistry) -> Command {
    let download = Command::new("download")
        .about("Download one or more links")
        .arg(
            Arg::new("links")
                .help("Links to download")
                .action(ArgAction::Append)
                .num_args(1..)
                .required(true),
        )
        .arg(
            Arg::new("out_dir")
                .long("out-dir")
                .help("Output directory")
                .default_value("./downloads")
                .num_args(1),
        )
        .arg(
            Arg::new("concurrency")
                .long("concurrency")
                .help("Max concurrent fragments per item")
                .default_value("6")
                .num_args(1),
        )
        .arg(
            Arg::new("chunk_mb")
                .long("chunk-mb")
                .help("Chunk size in MB (for HTTP range)")
                .default_value("8")
                .num_args(1),
        );

    let download = registry.augment_download_command(download);

    Command::new("downloader")
        .about("Multi-fragment downloader (HTTP + GitHub resolver) - plugin based")
        .subcommand_required(true)
        .arg_required_else_help(true)
        .subcommand(download)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let registry = PluginRegistry::with_defaults();
    let app = build_cli(&registry);
    let matches = app.get_matches();

    match matches.subcommand() {
        Some(("download", m)) => {
            let out_dir: PathBuf = m.get_one::<String>("out_dir").unwrap().into();
            let concurrency: usize = m.get_one::<String>("concurrency").unwrap().parse()?;
            let chunk_mb: u64 = m.get_one::<String>("chunk_mb").unwrap().parse()?;

            tokio::fs::create_dir_all(&out_dir).await?;

            let mut cfg = DownloadCliConfig {
                headers: HashMap::new(),
                options: HashMap::new(),
                driver_ctx: DriverContext {
                    user_agent: "OrangeDownloader/0.1".to_string(),
                    timeout_secs: 60,
                    retries: 2,
                    retry_backoff_ms: 400,
                },
            };
            registry.apply_download_matches(m, &mut cfg)?;

            let engine = Engine::new(
                registry,
                out_dir.clone(),
                concurrency,
                chunk_mb * 1024 * 1024,
                cfg.driver_ctx.clone(),
            )
            .await?;

            let links: Vec<String> = m
                .get_many::<String>("links")
                .unwrap()
                .map(|s| s.to_string())
                .collect();

            let inputs: Vec<LinkInput> = links
                .into_iter()
                .map(|raw| LinkInput {
                    raw,
                    headers: cfg.headers.clone(),
                    options: cfg.options.clone(),
                })
                .collect();

            let job_id = engine.add_and_start(inputs).await?;
            println!("Job started: {}", job_id);

            let mut rx = engine.subscribe();
            let ui_job_id = job_id;
            let ui_task = tokio::spawn(async move {
                let mp = MultiProgress::new();
                let sty_pb = ProgressStyle::with_template("{spinner:.green} {prefix} {wide_msg}")
                    .unwrap()
                    .tick_chars("|/-\\ ");
                let sty_bar = ProgressStyle::with_template(
                    "{prefix} {bar:40.cyan/blue} {bytes}/{total_bytes} ({bytes_per_sec}, eta {eta}) {wide_msg}",
                )
                .unwrap();

                #[derive(Clone)]
                struct ItemView {
                    display_name: String,
                    target_path: String,
                    uri: String,
                    status: String,
                    downloaded: u64,
                    total: Option<u64>,
                    errors: Vec<String>,
                }

                let mut bars: HashMap<Uuid, ProgressBar> = HashMap::new();
                let mut items: HashMap<Uuid, ItemView> = HashMap::new();

                loop {
                    let evt = match rx.recv().await {
                        Ok(e) => e,
                        Err(_) => break,
                    };

                    match evt {
                        EngineEvent::JobStatusChanged { job_id, status } => {
                            if job_id == ui_job_id {
                                let _ = mp.println(format!("[JOB] {} -> {:?}", job_id, status));
                                if matches!(status, core::model::JobStatus::Completed | core::model::JobStatus::Failed) {
                                    let _ = mp.println("".to_string());
                                    let _ = mp.println("Summary:".to_string());
                                    let mut ids: Vec<_> = items.keys().cloned().collect();
                                    ids.sort();
                                    for id in ids {
                                        if let Some(v) = items.get(&id) {
                                            let total_s = v.total.map(fmt_bytes).unwrap_or_else(|| "?".to_string());
                                            let _ = mp.println(format!(
                                                "- item={} status={} {} / {} name={} path={} uri={}",
                                                id,
                                                v.status,
                                                fmt_bytes(v.downloaded),
                                                total_s,
                                                v.display_name,
                                                v.target_path,
                                                v.uri,
                                            ));
                                            for e in &v.errors {
                                                let _ = mp.println(format!("  error: {}", e));
                                            }
                                        }
                                    }
                                    break;
                                }
                            }
                        }
                        EngineEvent::ItemAdded { item_id, display_name, target_path, uri } => {
                            let pb = mp.add(ProgressBar::new_spinner());
                            pb.set_style(sty_pb.clone());
                            pb.set_prefix(format!("[{display_name}]"));
                            pb.enable_steady_tick(std::time::Duration::from_millis(120));
                            pb.set_message(format!("added -> {} ({})", target_path.display(), uri));
                            bars.insert(item_id, pb);
                            items.insert(
                                item_id,
                                ItemView {
                                    display_name,
                                    target_path: target_path.display().to_string(),
                                    uri,
                                    status: "added".to_string(),
                                    downloaded: 0,
                                    total: None,
                                    errors: vec![],
                                },
                            );
                        }
                        EngineEvent::ItemStatusChanged { item_id, status } => {
                            if let Some(v) = items.get_mut(&item_id) {
                                v.status = format!("{:?}", status);
                            }
                            if let Some(pb) = bars.get(&item_id) {
                                pb.set_message(format!("status={:?}", status));
                                if matches!(status, core::model::ItemStatus::Done) {
                                    pb.finish_with_message("done".to_string());
                                }
                                if matches!(status, core::model::ItemStatus::Failed) {
                                    pb.finish_with_message("failed".to_string());
                                }
                            }
                        }
                        EngineEvent::Progress { item_id, downloaded, total, speed_bps, eta } => {
                            if let Some(v) = items.get_mut(&item_id) {
                                v.downloaded = downloaded;
                                v.total = total;
                            }
                            let pb = match bars.get(&item_id) {
                                Some(pb) => pb,
                                None => continue,
                            };

                            if let Some(t) = total {
                                if pb.length().unwrap_or(0) != t {
                                    pb.set_style(sty_bar.clone());
                                    pb.set_length(t);
                                }
                                pb.set_position(downloaded.min(t));
                            } else {
                                pb.set_style(sty_pb.clone());
                            }

                            let eta_s = eta
                                .map(|d| format!("{:.0}s", d.as_secs_f64()))
                                .unwrap_or_else(|| "-".to_string());
                            pb.set_message(format!("{} / {} | {} | eta {}", fmt_bytes(downloaded), total.map(fmt_bytes).unwrap_or_else(|| "?".to_string()), fmt_bytes(speed_bps), eta_s));
                        }
                        EngineEvent::FragmentDone { item_id, completed, total } => {
                            if let Some(pb) = bars.get(&item_id) {
                                pb.set_message(format!("fragments {}/{}", completed, total));
                            }
                        }
                        EngineEvent::Error { scope, message } => {
                            let _ = mp.println(format!("[ERR] {}: {}", scope, message));
                            for (id, v) in items.iter_mut() {
                                if scope.contains(&id.to_string()) {
                                    v.errors.push(format!("{}: {}", scope, message));
                                }
                            }
                        }
                        EngineEvent::Info { scope, message } => {
                            let _ = mp.println(format!("[INFO] {}: {}", scope, message));
                        }
                    }
                }
            });

            engine.wait_job(job_id).await;

            let _ = ui_task.await;

            println!("Job finished: {}", job_id);
        }
        _ => {}
    }

    Ok(())
}

fn fmt_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let f = n as f64;
    if f >= GB {
        format!("{:.2}GiB", f / GB)
    } else if f >= MB {
        format!("{:.2}MiB", f / MB)
    } else if f >= KB {
        format!("{:.2}KiB", f / KB)
    } else {
        format!("{}B", n)
    }
}
