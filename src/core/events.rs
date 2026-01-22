use crate::core::model::{ItemId, ItemStatus, JobId, JobStatus};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum EngineEvent {
    JobStatusChanged { job_id: JobId, status: JobStatus },
    ItemAdded { item_id: ItemId, display_name: String, target_path: PathBuf, uri: String },
    ItemStatusChanged { item_id: ItemId, status: ItemStatus },
    Progress {
        item_id: ItemId,
        downloaded: u64,
        total: Option<u64>,
        speed_bps: u64,
        eta: Option<Duration>,
    },
    FragmentDone { item_id: ItemId, completed: u64, total: u64 },
    Error { scope: String, message: String },
    Info { scope: String, message: String },
}
