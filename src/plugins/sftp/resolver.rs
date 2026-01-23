use async_trait::async_trait;
use crate::plugins::registry::{DownloadItemDraft, LinkResolver, ResolveContext, ResolveResult};
use crate::core::model::{Capabilities, LinkInput, ResourceDescriptor, ResourceType};
use sanitize_filename::sanitize;
use url::Url;

pub struct SftpResolver;

impl SftpResolver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LinkResolver for SftpResolver {
    fn name(&self) -> &'static str {
        "sftp-resolver"
    }

    fn can_handle(&self, input: &LinkInput) -> u8 {
        if let Ok(u) = Url::parse(&input.raw) {
            if u.scheme() == "sftp" {
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

        let res = ResourceDescriptor {
            rtype: ResourceType::Sftp,
            uri: input.raw.clone(),
            headers: Default::default(),
            meta: Default::default(),
            caps: Capabilities { supports_ranges: false, max_parallel: 1 },
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
            warnings: vec![
                "SFTP downloads use system scp. Configure key-based auth; password prompts are not supported.".into(),
            ],
        })
    }
}
