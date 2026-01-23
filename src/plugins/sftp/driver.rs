use crate::core::model::ResourceDescriptor;
use crate::plugins::registry::DriverContext;
use anyhow::Context;
use std::path::Path;
use tokio::process::Command;
use url::Url;

pub struct SftpDriver;

impl SftpDriver {
    pub fn new() -> Self {
        Self
    }

    pub async fn download_to_file(
        &self,
        res: &ResourceDescriptor,
        _ctx: &DriverContext,
        target_path: &Path,
        options: &std::collections::HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let url = Url::parse(&res.uri).context("parse sftp url")?;
        let host = url.host_str().context("sftp url missing host")?;
        let path = url.path();
        if path.is_empty() {
            anyhow::bail!("sftp url missing path: {}", res.uri);
        }

        let port: u16 = options
            .get("sftp_port")
            .and_then(|s| s.parse().ok())
            .or_else(|| url.port())
            .unwrap_or(22);

        let user = options
            .get("sftp_user")
            .cloned()
            .unwrap_or_else(|| {
                if url.username().is_empty() {
                    "".to_string()
                } else {
                    url.username().to_string()
                }
            });

        let identity = options.get("sftp_identity").cloned();

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let remote = if user.is_empty() {
            format!("{}:{}", host, path)
        } else {
            format!("{}@{}:{}", user, host, path)
        };

        let tmp_path = target_path.with_extension("partial");

        let mut cmd = Command::new("scp");
        cmd.arg("-B");
        cmd.arg("-P").arg(port.to_string());
        cmd.arg("-o").arg("BatchMode=yes");
        if let Some(id) = identity {
            cmd.arg("-i").arg(id);
        }
        cmd.arg(remote);
        cmd.arg(&tmp_path);

        let out = cmd.output().await.context("spawn scp")?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            anyhow::bail!("scp failed: {}", stderr.trim());
        }

        if tokio::fs::metadata(target_path).await.is_ok() {
            let _ = tokio::fs::remove_file(target_path).await;
        }
        tokio::fs::rename(&tmp_path, target_path).await?;

        Ok(())
    }
}
