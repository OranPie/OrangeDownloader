use crate::core::model::ResourceDescriptor;
use crate::plugins::registry::DriverContext;
use anyhow::Context;
use std::path::Path;
use tokio::process::Command;

pub struct Ed2kDriver;

impl Ed2kDriver {
    pub fn new() -> Self {
        Self
    }

    pub async fn download_to_path(
        &self,
        res: &ResourceDescriptor,
        _ctx: &DriverContext,
        target_path: &Path,
        options: &std::collections::HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let cmd = options
            .get("ed2k_cmd")
            .cloned()
            .filter(|s| !s.trim().is_empty())
            .context("ED2K requires --ed2k-cmd")?;

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let name = res.meta.get("name").cloned().unwrap_or_default();
        let size = res.meta.get("size").cloned().unwrap_or_default();
        let hash = res.meta.get("hash").cloned().unwrap_or_default();

        let mut args: Vec<String> = vec![];
        if let Some(raw_args) = options.get("ed2k_args") {
            for a in raw_args.split('\n').filter(|s| !s.trim().is_empty()) {
                args.push(a.to_string());
            }
        }

        let replace = |s: &str| {
            s.replace("{url}", &res.uri)
                .replace("{out}", &target_path.display().to_string())
                .replace("{name}", &name)
                .replace("{size}", &size)
                .replace("{hash}", &hash)
        };

        let mut proc = Command::new(replace(&cmd));
        for a in args {
            proc.arg(replace(&a));
        }

        let out = proc.output().await.context("spawn ed2k command")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            anyhow::bail!("ed2k command failed: {}", stderr.trim());
        }

        Ok(())
    }
}
