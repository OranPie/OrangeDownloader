use async_trait::async_trait;
use crate::plugins::registry::{DownloadItemDraft, LinkResolver, ResolveContext, ResolveResult};
use crate::core::model::{Capabilities, LinkInput, ResourceDescriptor, ResourceType};
use sanitize_filename::sanitize;
use std::collections::HashMap;
use url::Url;

pub struct BtResolver;
impl BtResolver { pub fn new() -> Self { Self } }

fn parse_btih(magnet: &Url) -> Option<String> {
    let qs = magnet.query_pairs().collect::<Vec<_>>();
    for (k, v) in qs {
        if k == "xt" {
            // xt=urn:btih:<hash>
            let s = v.to_string();
            if let Some(rest) = s.strip_prefix("urn:btih:") {
                return Some(rest.to_string());
            }
        }
    }
    None
}

fn parse_trackers(magnet: &Url) -> Vec<String> {
    magnet
        .query_pairs()
        .filter(|(k, _)| k == "tr")
        .map(|(_, v)| v.to_string())
        .collect()
}

#[async_trait]
impl LinkResolver for BtResolver {
    fn name(&self) -> &'static str { "bt-resolver" }

    fn can_handle(&self, input: &LinkInput) -> u8 {
        if input.raw.starts_with("magnet:") { 80 } else { 0 }
    }

    async fn resolve(&self, input: &LinkInput, ctx: &ResolveContext) -> anyhow::Result<ResolveResult> {
        let u = Url::parse(&input.raw)?;
        let infohash = parse_btih(&u).ok_or_else(|| anyhow::anyhow!("magnet missing xt=urn:btih:..."))?;
        let trackers = parse_trackers(&u);

        // display name：dn=xxx (可选)
        let dn = u.query_pairs()
            .find(|(k, _)| k == "dn")
            .map(|(_, v)| sanitize(&v))
            .unwrap_or_else(|| format!("torrent-{}", &infohash[..infohash.len().min(12)]));

        let mut meta = HashMap::new();
        meta.insert("infohash".into(), infohash);
        if !trackers.is_empty() {
            meta.insert("trackers".into(), trackers.join("\n"));
        }

        let res = ResourceDescriptor {
            rtype: ResourceType::BitTorrent,
            uri: input.raw.clone(),
            headers: Default::default(),
            meta,
            caps: Capabilities { supports_ranges: false, max_parallel: 0 },
        };

        Ok(ResolveResult {
            drafts: vec![DownloadItemDraft {
                display_name: dn.clone(),
                suggested_path: ctx.out_dir.join(&dn),
                total_size: None,
                resources: vec![res],
            }],
            warnings: vec![],
        })
    }
}
