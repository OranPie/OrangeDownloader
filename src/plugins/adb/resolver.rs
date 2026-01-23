use async_trait::async_trait;
use crate::plugins::registry::{DownloadItemDraft, LinkResolver, ResolveContext, ResolveResult};
use crate::core::model::{Capabilities, LinkInput, ResourceDescriptor, ResourceType};
use sanitize_filename::sanitize;
use url::Url;

pub struct AdbResolver;

impl AdbResolver {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl LinkResolver for AdbResolver {
    fn name(&self) -> &'static str {
        "adb-resolver"
    }

    fn can_handle(&self, input: &LinkInput) -> u8 {
        if let Ok(u) = Url::parse(&input.raw) {
            if u.scheme() == "adb" {
                return 70;
            }
        }
        0
    }

    async fn resolve(&self, input: &LinkInput, ctx: &ResolveContext) -> anyhow::Result<ResolveResult> {
        let url = Url::parse(&input.raw)?;
        // adb://<ignored-host>/<device_path>
        let device_path = url.path();
        if device_path.is_empty() || device_path == "/" {
            anyhow::bail!("adb url missing device path: {}", input.raw);
        }

        let filename = url
            .path_segments()
            .and_then(|s| s.last())
            .filter(|s| !s.is_empty())
            .map(|s| sanitize(s))
            .unwrap_or_else(|| "device.bin".to_string());

        let suggested_path = ctx.out_dir.join(filename);

        let mut meta = std::collections::HashMap::new();
        meta.insert("device_path".to_string(), device_path.to_string());

        let res = ResourceDescriptor {
            rtype: ResourceType::Adb,
            uri: input.raw.clone(),
            headers: Default::default(),
            meta,
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
                "ADB pull uses local adb binary; ensure a device is connected and authorized.".into(),
            ],
        })
    }
}
