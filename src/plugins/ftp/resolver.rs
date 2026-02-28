use async_trait::async_trait;
use crate::plugins::registry::{DownloadItemDraft, LinkResolver, ResolveContext, ResolveResult};
use crate::core::model::{Capabilities, LinkInput, ResourceDescriptor, ResourceType};
use sanitize_filename::sanitize;
use url::Url;

pub struct FtpResolver;

impl FtpResolver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LinkResolver for FtpResolver {
    fn name(&self) -> &'static str {
        "ftp-resolver"
    }

    fn can_handle(&self, input: &LinkInput) -> u8 {
        if let Ok(u) = Url::parse(&input.raw) {
            if u.scheme() == "ftp" {
                return 70;
            }
        }
        0
    }

    async fn resolve(&self, input: &LinkInput, ctx: &ResolveContext) -> anyhow::Result<ResolveResult> {
        let url = Url::parse(&input.raw)?;

        let filename = url
            .path_segments()
            .and_then(|s| s.last())
            .filter(|s| !s.is_empty())
            .map(|s| sanitize(s))
            .unwrap_or_else(|| "download.bin".to_string());

        let suggested_path = ctx.out_dir.join(filename);

        // Embed connection credentials into meta so the TransferDriver can access them
        // without needing a separate options map. CLI options override URL components.
        let mut meta = std::collections::HashMap::new();

        let user = input.options.get("ftp_user").cloned().unwrap_or_else(|| {
            if url.username().is_empty() {
                "anonymous".to_string()
            } else {
                url.username().to_string()
            }
        });
        meta.insert("ftp_user".to_string(), user);

        if let Some(pass) = input.options.get("ftp_pass").cloned()
            .or_else(|| url.password().map(|s| s.to_string()))
        {
            meta.insert("ftp_pass".to_string(), pass);
        }

        let port = input.options.get("ftp_port").cloned()
            .or_else(|| url.port().map(|p| p.to_string()))
            .unwrap_or_else(|| "21".to_string());
        meta.insert("ftp_port".to_string(), port);

        let res = ResourceDescriptor {
            rtype: ResourceType::Ftp,
            uri: input.raw.clone(),
            headers: Default::default(),
            meta,
            // FTP supports REST-based ranges; actual server capability is confirmed at probe time.
            caps: Capabilities { supports_ranges: true, max_parallel: 1 },
        };

        Ok(ResolveResult {
            drafts: vec![DownloadItemDraft {
                display_name: suggested_path
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
                suggested_path,
                total_size: None,
                resources: vec![res],
            }],
            warnings: vec![],
        })
    }
}
