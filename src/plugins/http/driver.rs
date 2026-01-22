use async_trait::async_trait;
use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT_RANGES, CONTENT_LENGTH, CONTENT_RANGE, RANGE, USER_AGENT};
use reqwest::StatusCode;
use std::time::Duration;
use tokio::time::sleep;

use crate::core::model::{ResourceDescriptor, ResourceType};
use crate::plugins::registry::{DriverContext, TransferDriver};

#[derive(thiserror::Error, Debug)]
pub enum HttpDriverError {
    #[error("range not supported by server (verified)")]
    RangeNotSupported,

    /// 服务器忽略 Range，直接返回 200 + 全量 body
    #[error("server ignored range and returned full content")]
    RangeIgnoredFull(Bytes),

    #[error("http status error: {0}")]
    Status(StatusCode),
}

pub struct HttpDriver {
    client: reqwest::Client,
}

impl HttpDriver {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::limited(10))
            .build()
            .expect("reqwest client");
        Self { client }
    }

    fn build_headers(res: &ResourceDescriptor, ctx: &DriverContext) -> anyhow::Result<HeaderMap> {
        let mut h = HeaderMap::new();
        h.insert(USER_AGENT, HeaderValue::from_str(&ctx.user_agent)?);
        for (k, v) in &res.headers {
            let name = HeaderName::from_bytes(k.as_bytes())?;
            let value = HeaderValue::from_str(v)?;
            h.insert(name, value);
        }
        Ok(h)
    }

    fn should_retry_status(status: StatusCode) -> bool {
        status == StatusCode::TOO_MANY_REQUESTS
            || status == StatusCode::REQUEST_TIMEOUT
            || status.is_server_error()
    }

    async fn sleep_backoff(ctx: &DriverContext, attempt: u32) {
        let base = ctx.retry_backoff_ms.max(1) as u64;
        let shift = attempt.min(16);
        let mul = 1u64 << shift;
        let ms = base.saturating_mul(mul).min(30_000);
        sleep(Duration::from_millis(ms)).await;
    }

    fn accept_ranges_hint(resp: &reqwest::Response) -> bool {
        resp.headers()
            .get(ACCEPT_RANGES)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_ascii_lowercase().contains("bytes"))
            .unwrap_or(false)
    }
}

#[async_trait]
impl TransferDriver for HttpDriver {
    fn name(&self) -> &'static str { "http-driver" }

    fn supports(&self, res: &ResourceDescriptor) -> bool {
        matches!(res.rtype, ResourceType::Http | ResourceType::GitHubResolvedHttp)
    }

    async fn prepare(&self, _res: &ResourceDescriptor, _ctx: &DriverContext) -> anyhow::Result<()> {
        Ok(())
    }

    /// ✅ 真 Range 探测：HEAD + GET bytes=0-0 => 必须 206 + Content-Range
    async fn probe(&self, res: &ResourceDescriptor, ctx: &DriverContext) -> anyhow::Result<(Option<u64>, bool)> {
        let headers = Self::build_headers(res, ctx)?;

        let head = self.client
            .head(&res.uri)
            .headers(headers.clone())
            .timeout(Duration::from_secs(ctx.timeout_secs))
            .send()
            .await?;

        let total = head.headers()
            .get(CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());

        let _hint = Self::accept_ranges_hint(&head);

        let test = self.client
            .get(&res.uri)
            .headers(headers)
            .timeout(Duration::from_secs(ctx.timeout_secs))
            .header(RANGE, "bytes=0-0")
            .send()
            .await?;

        let supports_ranges = test.status() == StatusCode::PARTIAL_CONTENT
            && test.headers().get(CONTENT_RANGE).is_some();

        Ok((total, supports_ranges))
    }

    async fn download_range(
        &self,
        res: &ResourceDescriptor,
        ctx: &DriverContext,
        start: u64,
        end_inclusive: u64,
    ) -> anyhow::Result<Bytes> {
        let headers = Self::build_headers(res, ctx)?;
        let range_value = format!("bytes={}-{}", start, end_inclusive);

        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..=ctx.retries {
            if attempt > 0 {
                Self::sleep_backoff(ctx, attempt - 1).await;
            }

            let resp = match self.client
                .get(&res.uri)
                .headers(headers.clone())
                .timeout(Duration::from_secs(ctx.timeout_secs))
                .header(RANGE, range_value.clone())
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(e.into());
                    continue;
                }
            };

            match resp.status() {
                StatusCode::PARTIAL_CONTENT => return Ok(resp.bytes().await?),

                // ✅ 关键：Range 被忽略 => 200 + 全量
                StatusCode::OK => {
                    let full = resp.bytes().await?;
                    return Err(HttpDriverError::RangeIgnoredFull(full).into());
                }

                StatusCode::RANGE_NOT_SATISFIABLE => return Err(HttpDriverError::RangeNotSupported.into()),

                s if s.is_success() => {
                    // 其它成功码但不是 206/200，不常见，按不支持处理更安全
                    return Err(HttpDriverError::RangeNotSupported.into());
                }

                s if Self::should_retry_status(s) => {
                    last_err = Some(HttpDriverError::Status(s).into());
                    continue;
                }

                s => return Err(HttpDriverError::Status(s).into()),
            }
        }

        Err(last_err.unwrap_or_else(|| HttpDriverError::Status(StatusCode::REQUEST_TIMEOUT).into()))
    }

    async fn download_all(&self, res: &ResourceDescriptor, ctx: &DriverContext) -> anyhow::Result<Bytes> {
        let headers = Self::build_headers(res, ctx)?;

        let mut last_err: Option<anyhow::Error> = None;
        for attempt in 0..=ctx.retries {
            if attempt > 0 {
                Self::sleep_backoff(ctx, attempt - 1).await;
            }

            let resp = match self.client
                .get(&res.uri)
                .headers(headers.clone())
                .timeout(Duration::from_secs(ctx.timeout_secs))
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    last_err = Some(e.into());
                    continue;
                }
            };

            if resp.status().is_success() {
                return Ok(resp.bytes().await?);
            }

            if Self::should_retry_status(resp.status()) {
                last_err = Some(HttpDriverError::Status(resp.status()).into());
                continue;
            }
            return Err(HttpDriverError::Status(resp.status()).into());
        }

        Err(last_err.unwrap_or_else(|| HttpDriverError::Status(StatusCode::REQUEST_TIMEOUT).into()))
    }
}
