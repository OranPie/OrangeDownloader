use async_trait::async_trait;
use crate::plugins::registry::{DownloadItemDraft, LinkResolver, ResolveContext, ResolveResult};
use crate::core::model::{Capabilities, LinkInput, ResourceDescriptor, ResourceType};
use sanitize_filename::sanitize;
use std::collections::HashMap;

pub struct Ed2kResolver;
impl Ed2kResolver { pub fn new() -> Self { Self } }

fn parse_ed2k_file(raw: &str) -> anyhow::Result<(String, u64, String)> {
    // Typical: ed2k://|file|NAME|SIZE|HASH|/
    let lower = raw.to_ascii_lowercase();
    if !lower.starts_with("ed2k://") {
        anyhow::bail!("not an ed2k url");
    }

    let s = raw.trim_start_matches("ed2k://");
    let s = s.trim_start_matches('|');
    let parts: Vec<&str> = s.split('|').collect();
    if parts.len() < 5 {
        anyhow::bail!("invalid ed2k url (too few fields)");
    }
    if parts[0].to_ascii_lowercase() != "file" {
        anyhow::bail!("ed2k only supports file links for now");
    }

    let name = parts[1].to_string();
    let size: u64 = parts[2].parse()?;
    let hash = parts[3].to_string();
    Ok((name, size, hash))
}

#[async_trait]
impl LinkResolver for Ed2kResolver {
    fn name(&self) -> &'static str { "ed2k-resolver" }

    fn can_handle(&self, input: &LinkInput) -> u8 {
        if input.raw.to_ascii_lowercase().starts_with("ed2k://") { 80 } else { 0 }
    }

    async fn resolve(&self, input: &LinkInput, ctx: &ResolveContext) -> anyhow::Result<ResolveResult> {
        let (name, size, hash) = parse_ed2k_file(&input.raw)?;
        let dn = if name.trim().is_empty() {
            format!("ed2k-{}", &hash[..hash.len().min(12)])
        } else {
            sanitize(&name)
        };

        let mut meta = HashMap::new();
        meta.insert("name".into(), name);
        meta.insert("size".into(), size.to_string());
        meta.insert("hash".into(), hash);

        let res = ResourceDescriptor {
            rtype: ResourceType::Ed2k,
            uri: input.raw.clone(),
            headers: Default::default(),
            meta,
            caps: Capabilities { supports_ranges: false, max_parallel: 1 },
        };

        Ok(ResolveResult {
            drafts: vec![DownloadItemDraft {
                display_name: dn.clone(),
                suggested_path: ctx.out_dir.join(&dn),
                total_size: Some(size),
                resources: vec![res],
            }],
            warnings: vec!["ED2K requires an external client command (see --ed2k-cmd).".into()],
        })
    }
}