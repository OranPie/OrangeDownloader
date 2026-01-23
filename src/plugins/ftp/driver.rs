use crate::core::model::ResourceDescriptor;
use crate::plugins::registry::DriverContext;
use anyhow::Context;
use async_ftp::FtpStream;
use tokio::fs::File;
use tokio::io::{self, AsyncWriteExt};
use url::Url;

pub struct FtpDriver;

impl FtpDriver {
    pub fn new() -> Self {
        Self
    }

    pub async fn download_to_file(
        &self,
        res: &ResourceDescriptor,
        _ctx: &DriverContext,
        target_path: &std::path::Path,
        options: &std::collections::HashMap<String, String>,
    ) -> anyhow::Result<()> {
        let url = Url::parse(&res.uri).context("parse ftp url")?;
        let host = url.host_str().context("ftp url missing host")?;
        let port: u16 = url.port().unwrap_or_else(|| {
            options
                .get("ftp_port")
                .and_then(|s| s.parse().ok())
                .unwrap_or(21)
        });

        let user = options.get("ftp_user").cloned().unwrap_or_else(|| {
            if url.username().is_empty() {
                "anonymous".to_string()
            } else {
                url.username().to_string()
            }
        });

        let pass = options
            .get("ftp_pass")
            .cloned()
            .or_else(|| url.password().map(|s| s.to_string()))
            .unwrap_or_default();

        let path = url.path().trim_start_matches('/');
        if path.is_empty() {
            anyhow::bail!("ftp url missing path: {}", res.uri);
        }

        let mut ftp = FtpStream::connect((host, port)).await.context("ftp connect")?;
        ftp.login(&user, &pass).await.context("ftp login")?;

        if let Some(parent) = target_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let tmp_path = target_path.with_extension("partial");
        let tmp_path_for_cb = tmp_path.clone();

        ftp.retr(path, move |mut reader| {
            let tmp_path_for_cb = tmp_path_for_cb.clone();
            async move {
                let mut file = File::create(&tmp_path_for_cb).await?;
                io::copy(&mut reader, &mut file).await?;
                file.flush().await?;
                Ok::<(), anyhow::Error>(())
            }
        })
        .await
        .context("ftp retr")?;

        let _ = ftp.quit().await;

        if tokio::fs::metadata(target_path).await.is_ok() {
            let _ = tokio::fs::remove_file(target_path).await;
        }
        tokio::fs::rename(&tmp_path, target_path).await?;

        Ok(())
    }
}
