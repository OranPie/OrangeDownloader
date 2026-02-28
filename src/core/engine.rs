use crate::core::assembler::Assembler;
use crate::core::events::EngineEvent;
use crate::core::model::*;
use crate::core::planner::plan_ranges;
use crate::core::store::SqliteStore;
use crate::plugins::registry::{DriverContext, PluginRegistry, ResolveContext};
use anyhow::Context;
use futures::stream::{FuturesUnordered, StreamExt};
use std::sync::Arc;
use tokio::sync::{broadcast, Mutex, Notify};
use tokio::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone)]
pub struct Engine {
    registry: Arc<PluginRegistry>,
    out_dir: std::path::PathBuf,
    concurrency: usize,
    chunk_size: u64,
    driver_ctx: DriverContext,
    event_tx: broadcast::Sender<EngineEvent>,
    jobs: Arc<Mutex<std::collections::HashMap<JobId, JobStatus>>>,
    job_notifies: Arc<Mutex<std::collections::HashMap<JobId, Arc<Notify>>>>,
    store: SqliteStore,
}

impl Engine {
    /// ✅ async ctor：不再 block_on
    pub async fn new(
        registry: PluginRegistry,
        out_dir: std::path::PathBuf,
        concurrency: usize,
        chunk_size: u64,
        driver_ctx: DriverContext,
    ) -> anyhow::Result<Self> {
        let (event_tx, _) = broadcast::channel(256);

        tokio::fs::create_dir_all(&out_dir).await
            .with_context(|| format!("create out_dir {}", out_dir.display()))?;

        let db_path = out_dir.join(".downloader.sqlite");
        let store = SqliteStore::open(&db_path).await?;

        Ok(Self {
            registry: Arc::new(registry),
            out_dir,
            concurrency: concurrency.max(1),
            chunk_size: chunk_size.max(1024 * 1024),
            driver_ctx,
            event_tx,
            jobs: Arc::new(Mutex::new(std::collections::HashMap::new())),
            job_notifies: Arc::new(Mutex::new(std::collections::HashMap::new())),
            store,
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.event_tx.subscribe()
    }

    pub async fn add_and_start(&self, inputs: Vec<LinkInput>) -> anyhow::Result<JobId> {
        let job_id = Uuid::new_v4();
        {
            let mut jobs = self.jobs.lock().await;
            jobs.insert(job_id, JobStatus::Pending);
        }
        let _ = self.event_tx.send(EngineEvent::JobStatusChanged { job_id, status: JobStatus::Pending });

        let notify = Arc::new(Notify::new());
        {
            let mut m = self.job_notifies.lock().await;
            m.insert(job_id, notify.clone());
        }

        let engine = self.clone();
        tokio::spawn(async move {
            engine.run_job(job_id, inputs, notify).await;
        });

        Ok(job_id)
    }

    pub async fn wait_job(&self, job_id: JobId) {
        let notify = {
            let m = self.job_notifies.lock().await;
            m.get(&job_id).cloned()
        };

        if let Some(n) = notify {
            n.notified().await;
        }
    }

    async fn run_job(&self, job_id: JobId, inputs: Vec<LinkInput>, notify: Arc<Notify>) {
        {
            let mut jobs = self.jobs.lock().await;
            jobs.insert(job_id, JobStatus::Running);
        }
        let _ = self.event_tx.send(EngineEvent::JobStatusChanged { job_id, status: JobStatus::Running });

        let ctx = ResolveContext { out_dir: self.out_dir.clone(), user_agent: self.driver_ctx.user_agent.clone() };

        let mut items: Vec<DownloadItem> = vec![];
        let mut any_failed = false;
        for input in inputs {
            let input_options = input.options.clone();
            let resolver = match self.registry.best_resolver(&input) {
                Some(r) => r,
                None => {
                    any_failed = true;
                    let _ = self.event_tx.send(EngineEvent::Error {
                        scope: "resolve".to_string(),
                        message: format!("no resolver for input: {}", input.raw),
                    });
                    continue;
                }
            };

            let _ = self.event_tx.send(EngineEvent::Info {
                scope: "resolve".to_string(),
                message: format!("input={} resolver={}", input.raw, resolver.name()),
            });

            let rr = resolver.resolve(&input, &ctx).await;
            match rr {
                Ok(resolved) => {
                    for w in &resolved.warnings {
                        let _ = self.event_tx.send(EngineEvent::Info {
                            scope: "resolve-warning".to_string(),
                            message: w.clone(),
                        });
                    }
                    for d in resolved.drafts {
                        let item_id = Uuid::new_v4();
                        let item = DownloadItem {
                            id: item_id,
                            job_id,
                            status: ItemStatus::Ready,
                            display_name: d.display_name,
                            target_path: d.suggested_path,
                            total_size: d.total_size,
                            resources: d.resources,
                            options: input_options.clone(),
                            fragments: vec![],
                        };
                        if let Some(res0) = item.resources.get(0) {
                            let _ = self.event_tx.send(EngineEvent::ItemAdded {
                                item_id,
                                display_name: item.display_name.clone(),
                                target_path: item.target_path.clone(),
                                uri: res0.uri.clone(),
                            });
                        }
                        items.push(item);
                    }
                }
                Err(e) => {
                    any_failed = true;
                    let _ = self.event_tx.send(EngineEvent::Error {
                        scope: format!("resolve({})", resolver.name()),
                        message: format!("{:#}", e),
                    });
                }
            }
        }

        for mut item in items {
            let r = self.download_item(&mut item).await;
            match r {
                Ok(_) => {
                    let _ = self.event_tx.send(EngineEvent::ItemStatusChanged { item_id: item.id, status: ItemStatus::Done });
                }
                Err(e) => {
                    any_failed = true;
                    let _ = self.event_tx.send(EngineEvent::Error {
                        scope: format!("item({})", item.display_name),
                        message: format!("{:#}", e),
                    });
                    let _ = self.event_tx.send(EngineEvent::ItemStatusChanged { item_id: item.id, status: ItemStatus::Failed });
                }
            }
        }

        {
            let mut jobs = self.jobs.lock().await;
            jobs.insert(job_id, if any_failed { JobStatus::Failed } else { JobStatus::Completed });
        }
        let _ = self.event_tx.send(EngineEvent::JobStatusChanged {
            job_id,
            status: if any_failed { JobStatus::Failed } else { JobStatus::Completed },
        });

        {
            let mut m = self.job_notifies.lock().await;
            m.remove(&job_id);
        }
        notify.notify_waiters();
    }

    pub async fn is_job_finished(&self, job_id: JobId) -> bool {
        let jobs = self.jobs.lock().await;
        matches!(jobs.get(&job_id), Some(JobStatus::Completed | JobStatus::Failed))
    }

    async fn download_item(&self, item: &mut DownloadItem) -> anyhow::Result<()> {
        let _ = self.event_tx.send(EngineEvent::ItemStatusChanged { item_id: item.id, status: ItemStatus::Downloading });

        let res = item.resources.get(0).context("no resource")?.clone();
        if matches!(res.rtype, ResourceType::BitTorrent) {
            let info = res.meta.get("infohash").cloned().unwrap_or_default();
            let _ = self.event_tx.send(EngineEvent::Info {
                scope: format!("bt item={}", item.display_name),
                message: format!("starting magnet download. infohash={}", info),
            });

            crate::plugins::bt::driver::BtDriver::new()
                .download_magnet_to_dir(&res, &self.driver_ctx, &item.target_path)
                .await?;

            let _ = self.event_tx.send(EngineEvent::Info {
                scope: format!("bt item={}", item.display_name),
                message: "completed".to_string(),
            });
            return Ok(());
        }

        if matches!(res.rtype, ResourceType::Adb) {
            let _ = self.event_tx.send(EngineEvent::Info {
                scope: format!("adb item={}", item.display_name),
                message: format!("pulling {}", res.uri),
            });

            crate::plugins::adb::driver::AdbDriver::new()
                .pull_to_file(&res, &self.driver_ctx, &item.target_path, &item.options)
                .await?;

            let _ = self.event_tx.send(EngineEvent::Info {
                scope: format!("adb item={}", item.display_name),
                message: "completed".to_string(),
            });
            return Ok(());
        }

        if matches!(res.rtype, ResourceType::Ed2k) {
            let hash = res.meta.get("hash").cloned().unwrap_or_default();
            let size = res.meta.get("size").cloned().unwrap_or_default();
            let _ = self.event_tx.send(EngineEvent::Info {
                scope: format!("ed2k item={}", item.display_name),
                message: format!("starting (hash={} size={})", hash, size),
            });

            crate::plugins::ed2k::driver::Ed2kDriver::new()
                .download_to_path(&res, &self.driver_ctx, &item.target_path, &item.options)
                .await?;

            let _ = self.event_tx.send(EngineEvent::Info {
                scope: format!("ed2k item={}", item.display_name),
                message: "completed".to_string(),
            });
            return Ok(());
        }

        if matches!(res.rtype, ResourceType::Sftp) {
            let _ = self.event_tx.send(EngineEvent::Info {
                scope: format!("sftp item={}", item.display_name),
                message: format!("downloading {}", res.uri),
            });

            crate::plugins::sftp::driver::SftpDriver::new()
                .download_to_file(&res, &self.driver_ctx, &item.target_path, &item.options)
                .await?;

            let _ = self.event_tx.send(EngineEvent::Info {
                scope: format!("sftp item={}", item.display_name),
                message: "completed".to_string(),
            });
            return Ok(());
        }

        let driver = self.registry.driver_for(&res).context("no driver for resource")?;
        let _ = self.event_tx.send(EngineEvent::Info {
            scope: format!("driver item={}", item.display_name),
            message: format!("selected driver={}", driver.name()),
        });
        let dctx = self.driver_ctx.clone();
        driver.prepare(&res, &dctx).await?;

        // probe（仍用临时 HttpDriver 做 HEAD；后续可做 trait 扩展）
        let (total_opt, supports_ranges) = driver.probe(&res, &dctx).await.unwrap_or((None, false));
        let _ = self.event_tx.send(EngineEvent::Info {
            scope: format!("probe item={}", item.display_name),
            message: format!("total={:?} supports_ranges={}", total_opt, supports_ranges),
        });

        item.total_size = total_opt;

        let partial_path = item.target_path.with_extension("partial");
        let item_rec = self.store
            .upsert_item(
                &res.uri,
                &item.target_path,
                &partial_path,
                self.chunk_size as i64,
                item.total_size.map(|v| v as i64),
                supports_ranges && item.total_size.is_some(),
            )
            .await?;

        // 规划并落库 fragments（存在就不覆盖）
        if supports_ranges && item.total_size.is_some() {
            let total = item.total_size.unwrap();
            let frags = plan_ranges(total, self.chunk_size);
            let ranges: Vec<(u64, u64)> = frags
                .iter()
                .map(|f| match f.key {
                    FragmentKey::Range { offset, len } => (offset, len),
                    _ => (0, 0),
                })
                .collect();
            self.store.ensure_fragments_for_ranges(item_rec.item_db_id, &ranges).await?;
        } else {
            self.store.ensure_fragments_for_ranges(item_rec.item_db_id, &[(0, 0)]).await?;
        }

        let db_frags = self.store.load_fragments(item_rec.item_db_id).await?;
        let total_frags = db_frags.len() as u64;
        let completed_init = db_frags.iter().filter(|f| f.state == FragmentState::Done).count() as u64;

        let assembler = Arc::new(Assembler::create(&partial_path, item.total_size).await?);

        let downloaded = Arc::new(Mutex::new(item_rec.downloaded_bytes.max(0) as u64));
        let completed_frags = Arc::new(Mutex::new(completed_init));

        let mut pending: Vec<usize> = db_frags
            .iter()
            .enumerate()
            .filter(|(_, f)| matches!(f.state, FragmentState::Missing | FragmentState::Bad))
            .map(|(i, _)| i)
            .collect();

        let start_time = Instant::now();

        while !pending.is_empty() {
            let batch: Vec<usize> = pending.drain(0..pending.len().min(self.concurrency)).collect();
            let mut futs = FuturesUnordered::new();

            for idx in batch {
                let f = db_frags[idx].clone();
                let driver2 = driver.clone();
                let res2 = res.clone();
                let dctx2 = dctx.clone();
                let assembler2 = assembler.clone();
                let downloaded2 = downloaded.clone();
                let completed2 = completed_frags.clone();
                let tx = self.event_tx.clone();
                let store2 = self.store.clone();
                let item_db_id = item_rec.item_db_id;
                let item_id = item.id;
                let total = item.total_size;

                futs.push(async move {
                    store2.set_fragment_state(f.frag_db_id, FragmentState::Downloading).await.ok();

                    let (offset, len) = (f.offset as u64, f.len as u64);

                    let bytes = if len == 0 {
                        driver2.download_all(&res2, &dctx2).await?
                    } else {
                        let end = offset + len - 1;
                        driver2.download_range(&res2, &dctx2, offset, end).await?
                    };

                    assembler2.write_at(offset, &bytes).await?;
                    store2.mark_fragment_done_and_add_bytes(f.frag_db_id, item_db_id, bytes.len() as i64).await?;

                    let dnow = {
                        let mut d = downloaded2.lock().await;
                        *d += bytes.len() as u64;
                        *d
                    };

                    let cnow = {
                        let mut c = completed2.lock().await;
                        *c += 1;
                        let _ = tx.send(EngineEvent::FragmentDone {
                            item_id,
                            completed: *c,
                            total: total_frags,
                        });
                        *c
                    };

                    let elapsed = start_time.elapsed().as_secs_f64().max(0.001);
                    let speed = (dnow as f64 / elapsed) as u64;
                    let eta = match (total, speed) {
                        (Some(t), s) if s > 0 && dnow < t => Some(Duration::from_secs_f64(((t - dnow) as f64) / (s as f64))),
                        _ => None,
                    };

                    let _ = tx.send(EngineEvent::Progress {
                        item_id,
                        downloaded: dnow,
                        total,
                        speed_bps: speed,
                        eta,
                    });

                    Ok::<u64, anyhow::Error>(cnow)
                });
            }

            while let Some(res) = futs.next().await {
                if let Err(e) = res {
                    let _ = self.event_tx.send(EngineEvent::Error {
                        scope: format!("download_fragment(item={})", item.id),
                        message: format!("{:#}", e),
                    });
                    anyhow::bail!(e);
                }
            }
        }

        assembler.flush().await?;

        let _ = self.event_tx.send(EngineEvent::ItemStatusChanged { item_id: item.id, status: ItemStatus::Assembling });

        let db_frags2 = self.store.load_fragments(item_rec.item_db_id).await?;
        if !db_frags2.iter().all(|f| f.state == FragmentState::Done) {
            anyhow::bail!("not all fragments completed (unexpected)");
        }

        if tokio::fs::metadata(&item.target_path).await.is_ok() {
            let _ = tokio::fs::remove_file(&item.target_path).await;
        }
        tokio::fs::rename(&partial_path, &item.target_path).await?;

        Ok(())
    }
}
