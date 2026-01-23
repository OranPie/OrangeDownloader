use crate::core::model::ResourceDescriptor;
use crate::plugins::registry::DriverContext;
use anyhow::Context;
use std::path::Path;
use tokio::process::Command;

pub struct AdbDriver;

impl AdbDriver {
    pub fn new() -> Self {
        Self
    }

    pub async fn pull_to_file(
        &self,
        res: &ResourceDescriptor,
        _ctx: &DriverContext,
        target_path: &Path,
        options: &std::collections::HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let adb_bin = options
            .get("adb_bin")
            .cloned()
            .unwrap_or_else(|| "adb".to_string());

        let serial = options.get("adb_serial").cloned();

        let device_path = res
            .meta
            .get("device_path")
            .cloned()
            .unwrap_or_else(|| {
                // fallback: adb://.../<path>
                let u = url::Url::parse(&res.uri).ok();
                u.map(|u| u.path().to_string()).unwrap_or_default()
            });
        if device_path.is_empty() || device_path == "/" {
            anyhow::bail!("adb missing device path");
        }

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let tmp_path = target_path.with_extension("partial");

        let mut cmd = Command::new(adb_bin);
        if let Some(s) = serial {
            cmd.arg("-s").arg(s);
        }
        cmd.arg("pull");
        cmd.arg(&device_path);
        cmd.arg(&tmp_path);

        let out = cmd.output().await.context("spawn adb pull")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            anyhow::bail!("adb pull failed: {}", stderr.trim());
        }

        if tokio::fs::metadata(target_path).await.is_ok() {
            let _ = tokio::fs::remove_file(target_path).await;
        }
        tokio::fs::rename(&tmp_path, target_path).await?;

        Ok(())
    }
}
