use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

pub type JobId = Uuid;
pub type ItemId = Uuid;

#[derive(Debug, Clone)]
pub struct LinkInput {
    pub raw: String,
    pub headers: HashMap<String, String>,
    pub options: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemStatus {
    Resolving,
    Ready,
    Downloading,
    Verifying,
    Assembling,
    Done,
    Failed,
}

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub id: JobId,
    pub status: JobStatus,
    pub items: Vec<DownloadItem>,
}

#[derive(Debug, Clone)]
pub struct DownloadItem {
    pub id: ItemId,
    pub job_id: JobId,
    pub status: ItemStatus,
    pub display_name: String,
    pub target_path: PathBuf,
    pub total_size: Option<u64>,
    pub resources: Vec<ResourceDescriptor>,
    pub fragments: Vec<Fragment>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceType {
    Http,
    GitHubResolvedHttp,
    BitTorrent,
    Ed2k,
}

#[derive(Debug, Clone, Default)]
pub struct Capabilities {
    pub supports_ranges: bool,
    pub max_parallel: u32,
}

#[derive(Debug, Clone)]
pub struct ResourceDescriptor {
    pub rtype: ResourceType,
    pub uri: String,
    pub headers: HashMap<String, String>,
    pub meta: HashMap<String, String>,
    pub caps: Capabilities,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentState {
    Missing,
    Downloading,
    Done,
    Bad,
}

#[derive(Debug, Clone)]
pub enum FragmentKey {
    Range { offset: u64, len: u64 },
    Indexed { index: u32 }, // 预留 BT/ED2K
}

#[derive(Debug, Clone)]
pub struct Fragment {
    pub key: FragmentKey,
    pub state: FragmentState,
    pub retry: u8,
}

pub enum FallbackMode { Full, SeqRange }
