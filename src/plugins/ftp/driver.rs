use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::AsyncReadExt;

use crate::core::model::{ResourceDescriptor, ResourceType};
use crate::plugins::registry::{DriverContext, TransferDriver};
use anyhow::Context;
use async_ftp::FtpStream;
use url::Url;
use std::time::Duration;
use tokio::time::sleep;

pub struct FtpDriver;

impl FtpDriver {
    pub fn new() -> Self {
        Self
    }

    /// Parse connection parameters from a ResourceDescriptor.
    /// Credentials are stored in `res.meta` by the FTP resolver; the URI provides
    /// host/path as fallback for any missing fields.
    fn parse_conn(res: &ResourceDescriptor) -> anyhow::Result<(String, u16, String, String, String)> {
        let url = Url::parse(&res.uri).context("parse ftp url")?;
        let host = url.host_str().context("ftp url missing host")?.to_string();

        let port: u16 = res.meta.get("ftp_port")
            .and_then(|s| s.parse().ok())
            .or_else(|| url.port())
            .unwrap_or(21);

        let user = res.meta.get("ftp_user").cloned().unwrap_or_else(|| {
            if url.username().is_empty() { "anonymous".to_string() } else { url.username().to_string() }
        });

        let pass = res.meta.get("ftp_pass").cloned()
            .or_else(|| url.password().map(|s| s.to_string()))
            .unwrap_or_default();

        let path = url.path().trim_start_matches('/').to_string();

        Ok((host, port, user, pass, path))
    }

    async fn connect(
        host: &str,
        port: u16,
        user: &str,
        pass: &str,
        ctx: &DriverContext,
    ) -> anyhow::Result<FtpStream> {
        let addr = format!("{}:{}", host, port);
        let mut ftp = tokio::time::timeout(
            Duration::from_secs(ctx.timeout_secs),
            FtpStream::connect(addr.as_str()),
        )
        .await
        .context("ftp connect timeout")?
        .context("ftp connect")?;

        ftp.login(user, pass).await.context("ftp login")?;
        Ok(ftp)
    }

    async fn sleep_backoff(ctx: &DriverContext, attempt: u32) {
        let base = ctx.retry_backoff_ms.max(1);
        // Cap the exponent at 16 to avoid overflow; this gives a maximum multiplier of 65536.
        // The resulting delay is then clamped to 30 000 ms (30 s) to prevent runaway waits.
        let shift = attempt.min(16);
        let ms = base.saturating_mul(1u64 << shift).min(30_000);
        sleep(Duration::from_millis(ms)).await;
    }
}

#[async_trait]
impl TransferDriver for FtpDriver {
    fn name(&self) -> &'static str { "ftp-driver" }

    fn supports(&self, res: &ResourceDescriptor) -> bool {
        matches!(res.rtype, ResourceType::Ftp)
    }

    /// Probe the FTP server: use the SIZE command to determine file size.
    /// Returns (Some(size), true) if the server supports SIZE and therefore
    /// REST-based range downloads; (None, false) otherwise.
    async fn probe(&self, res: &ResourceDescriptor, ctx: &DriverContext) -> anyhow::Result<(Option<u64>, bool)> {
        let (host, port, user, pass, path) = match Self::parse_conn(res) {
            Ok(v) => v,
            Err(_) => return Ok((None, false)),
        };
        if path.is_empty() {
            return Ok((None, false));
        }

        let result: anyhow::Result<(Option<u64>, bool)> = async {
            let mut ftp = Self::connect(&host, port, &user, &pass, ctx).await?;
            let file_size = ftp.size(&path).await.ok().flatten().map(|bytes| bytes as u64);
            let supports_ranges = file_size.is_some();
            let _ = ftp.quit().await;
            Ok((file_size, supports_ranges))
        }.await;

        Ok(result.unwrap_or((None, false)))
    }

    /// Download a byte range using the FTP REST+RETR commands.
    async fn download_range(
        &self,
        res: &ResourceDescriptor,
        ctx: &DriverContext,
        start: u64,
        end_inclusive: u64,
    ) -> anyhow::Result<Bytes> {
        let (host, port, user, pass, path) = Self::parse_conn(res)?;
        if path.is_empty() {
            anyhow::bail!("ftp url missing path: {}", res.uri);
        }

        let len = (end_inclusive - start + 1) as usize;
        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..=ctx.retries {
            if attempt > 0 {
                Self::sleep_backoff(ctx, attempt - 1).await;
            }

            let result: anyhow::Result<Bytes> = async {
                let mut ftp = Self::connect(&host, port, &user, &pass, ctx).await?;
                ftp.restart_from(start).await.context("ftp REST")?;
                let mut reader = ftp.get(&path).await.context("ftp RETR")?;
                let mut buf = vec![0u8; len];
                reader.read_exact(&mut buf).await.context("ftp read range")?;
                drop(reader);
                let _ = ftp.quit().await;
                Ok(Bytes::from(buf))
            }.await;

            match result {
                Ok(bytes) => return Ok(bytes),
                Err(e) => { last_err = Some(e); }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("ftp range download failed after retries")))
    }

    /// Download the entire file using RETR.
    async fn download_all(&self, res: &ResourceDescriptor, ctx: &DriverContext) -> anyhow::Result<Bytes> {
        let (host, port, user, pass, path) = Self::parse_conn(res)?;
        if path.is_empty() {
            anyhow::bail!("ftp url missing path: {}", res.uri);
        }

        let mut last_err: Option<anyhow::Error> = None;

        for attempt in 0..=ctx.retries {
            if attempt > 0 {
                Self::sleep_backoff(ctx, attempt - 1).await;
            }

            let result: anyhow::Result<Bytes> = async {
                let mut ftp = Self::connect(&host, port, &user, &pass, ctx).await?;
                let cursor = ftp.simple_retr(&path).await.context("ftp RETR")?;
                let _ = ftp.quit().await;
                Ok(Bytes::from(cursor.into_inner()))
            }.await;

            match result {
                Ok(bytes) => return Ok(bytes),
                Err(e) => { last_err = Some(e); }
            }
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("ftp download failed after retries")))
    }
}
