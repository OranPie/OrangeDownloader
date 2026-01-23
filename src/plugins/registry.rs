use async_trait::async_trait;
use crate::core::model::{LinkInput, ResourceDescriptor, ResourceType};
use clap::{ArgMatches, Command};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

#[derive(Debug)]
pub struct ResolveContext {
    pub out_dir: PathBuf,
    pub user_agent: String,
}

#[derive(Debug)]
pub struct ResolveResult {
    pub drafts: Vec<DownloadItemDraft>,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct DownloadItemDraft {
    pub display_name: String,
    pub suggested_path: PathBuf,
    pub total_size: Option<u64>,
    pub resources: Vec<ResourceDescriptor>,
}

#[async_trait]
pub trait LinkResolver: Send + Sync {
    fn name(&self) -> &'static str;
    fn can_handle(&self, input: &LinkInput) -> u8;
    async fn resolve(&self, input: &LinkInput, ctx: &ResolveContext) -> anyhow::Result<ResolveResult>;
}

#[derive(Debug, Clone)]
pub struct DownloadCliConfig {
    pub headers: HashMap<String, String>,
    pub options: HashMap<String, String>,
    pub driver_ctx: DriverContext,
}

pub trait CliPlugin: Send + Sync {
    fn name(&self) -> &'static str;
    fn augment_download_command(&self, cmd: Command) -> Command;
    fn apply_download_matches(&self, matches: &ArgMatches, cfg: &mut DownloadCliConfig) -> anyhow::Result<()>;
}

#[derive(Debug, Clone)]
pub struct DriverContext {
    pub user_agent: String,
    pub timeout_secs: u64,
    pub retries: u32,
    pub retry_backoff_ms: u64,
}

#[async_trait]
pub trait TransferDriver: Send + Sync {
    fn name(&self) -> &'static str;
    fn supports(&self, res: &ResourceDescriptor) -> bool;

    /// 可选：做 connection pool/认证等初始化；骨架里不强制用
    async fn prepare(&self, _res: &ResourceDescriptor, _ctx: &DriverContext) -> anyhow::Result<()> {
        Ok(())
    }

    async fn download_range(
        &self,
        res: &ResourceDescriptor,
        ctx: &DriverContext,
        start: u64,
        end_inclusive: u64,
    ) -> anyhow::Result<bytes::Bytes>;

    async fn download_all(&self, res: &ResourceDescriptor, ctx: &DriverContext) -> anyhow::Result<bytes::Bytes>;

    /// 可选：探测资源（大小、Range 支持）。默认表示“未知/不支持”。
    async fn probe(&self, _res: &ResourceDescriptor, _ctx: &DriverContext) -> anyhow::Result<(Option<u64>, bool)> {
        Ok((None, false))
    }

}

pub struct PluginRegistry {
    resolvers: Vec<Box<dyn LinkResolver>>,
    drivers: Vec<Arc<dyn TransferDriver>>,
    cli_plugins: Vec<Box<dyn CliPlugin>>,
}

impl PluginRegistry {
    pub fn with_defaults() -> Self {
        let mut reg = Self { resolvers: vec![], drivers: vec![], cli_plugins: vec![] };

        reg.resolvers.push(Box::new(crate::plugins::github::resolver::GitHubResolver::new()));
        reg.resolvers.push(Box::new(crate::plugins::http::resolver::HttpResolver::new()));
        reg.resolvers.push(Box::new(crate::plugins::bt::resolver::BtResolver::new()));
        reg.resolvers.push(Box::new(crate::plugins::ed2k::resolver::Ed2kResolver::new()));
        reg.resolvers.push(Box::new(crate::plugins::ftp::resolver::FtpResolver::new()));
        reg.resolvers.push(Box::new(crate::plugins::sftp::resolver::SftpResolver::new()));
        reg.resolvers.push(Box::new(crate::plugins::adb::resolver::AdbResolver::new()));

        reg.drivers.push(Arc::new(crate::plugins::http::driver::HttpDriver::new()));

        reg.cli_plugins.push(Box::new(crate::plugins::http::cli::HttpCliPlugin::new()));
        reg.cli_plugins.push(Box::new(crate::plugins::ed2k::cli::Ed2kCliPlugin::new()));
        reg.cli_plugins.push(Box::new(crate::plugins::ftp::cli::FtpCliPlugin::new()));
        reg.cli_plugins.push(Box::new(crate::plugins::sftp::cli::SftpCliPlugin::new()));
        reg.cli_plugins.push(Box::new(crate::plugins::adb::cli::AdbCliPlugin::new()));
        reg
    }

    pub fn augment_download_command(&self, cmd: Command) -> Command {
        self.cli_plugins
            .iter()
            .fold(cmd, |c, p| p.augment_download_command(c))
    }

    pub fn apply_download_matches(&self, matches: &ArgMatches, cfg: &mut DownloadCliConfig) -> anyhow::Result<()> {
        for p in &self.cli_plugins {
            p.apply_download_matches(matches, cfg)?;
        }
        Ok(())
    }

    pub fn best_resolver(&self, input: &LinkInput) -> Option<&dyn LinkResolver> {
        self.resolvers
            .iter()
            .map(|r| (r.can_handle(input), r.as_ref()))
            .max_by_key(|(c, _)| *c)
            .and_then(|(c, r)| if c == 0 { None } else { Some(r) })
    }

    pub fn driver_for(&self, res: &ResourceDescriptor) -> Option<Arc<dyn TransferDriver>> {
        self.drivers.iter().find(|d| d.supports(res)).cloned()
    }

    pub fn is_fragmented_http_like(res: &ResourceDescriptor) -> bool {
        matches!(res.rtype, ResourceType::Http | ResourceType::GitHubResolvedHttp)
    }
}
