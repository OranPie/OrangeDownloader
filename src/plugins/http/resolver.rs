use async_trait::async_trait;
use crate::plugins::registry::{DownloadItemDraft, LinkResolver, ResolveContext, ResolveResult};
use crate::core::model::{Capabilities, LinkInput, ResourceDescriptor, ResourceType};
use sanitize_filename::sanitize;
use url::Url;

pub struct HttpResolver;

impl HttpResolver {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl LinkResolver for HttpResolver {
    fn name(&self) -> &'static str { "http-resolver" }

    fn can_handle(&self, input: &LinkInput) -> u8 {
        if let Ok(u) = Url::parse(&input.raw) {
            if u.scheme() == "http" || u.scheme() == "https" { return 60; }
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

        // 先不做 HEAD 探测（由 driver 在 engine 内做更合适），这里给个基础资源
        let res = ResourceDescriptor {
            rtype: ResourceType::Http,
            uri: input.raw.clone(),
            headers: input.headers.clone(),
            meta: Default::default(),
            caps: Capabilities { supports_ranges: true, max_parallel: 8 },
        };

        Ok(ResolveResult {
            drafts: vec![DownloadItemDraft {
                display_name: suggested_path.file_name().unwrap().to_string_lossy().to_string(),
                suggested_path,
                total_size: None,
                resources: vec![res],
            }],
            warnings: vec![],
        })
    }
}
