use async_trait::async_trait;
use crate::plugins::registry::{DownloadItemDraft, LinkResolver, ResolveContext, ResolveResult};
use crate::core::model::{Capabilities, LinkInput, ResourceDescriptor, ResourceType};
use sanitize_filename::sanitize;
use url::Url;

pub struct GitHubResolver;

impl GitHubResolver {
    pub fn new() -> Self { Self }

    fn is_github_host(host: &str) -> bool {
        host.eq_ignore_ascii_case("github.com")
            || host.eq_ignore_ascii_case("raw.githubusercontent.com")
    }

    fn blob_to_raw(u: &Url) -> Option<Url> {
        // https://github.com/owner/repo/blob/branch/path -> https://raw.githubusercontent.com/owner/repo/branch/path
        if u.host_str()? != "github.com" { return None; }
        let seg: Vec<_> = u.path_segments()?.collect();
        if seg.len() < 5 { return None; }
        if seg[2] != "blob" { return None; }
        let owner = seg[0];
        let repo = seg[1];
        let branch = seg[3];
        let rest = &seg[4..];
        let mut raw = Url::parse("https://raw.githubusercontent.com/").ok()?;
        raw.set_path(&format!("{}/{}/{}/{}", owner, repo, branch, rest.join("/")));
        Some(raw)
    }

    fn repo_archive(u: &Url) -> Option<Url> {
        // https://github.com/owner/repo -> default to zip of HEAD (not perfect but works)
        // Prefer: https://github.com/owner/repo/archive/refs/heads/main.zip (needs branch)
        // Here: use /archive/refs/heads/master.zip fallback not always valid; so we keep original if not sure.
        let seg: Vec<_> = u.path_segments()?.collect();
        if seg.len() < 2 { return None; }
        if u.host_str()? != "github.com" { return None; }
        let owner = seg[0];
        let repo = seg[1];
        // try main.zip
        let mut zip = Url::parse("https://github.com/").ok()?;
        zip.set_path(&format!("{}/{}/archive/refs/heads/main.zip", owner, repo));
        Some(zip)
    }
}

#[async_trait]
impl LinkResolver for GitHubResolver {
    fn name(&self) -> &'static str { "github-resolver" }

    fn can_handle(&self, input: &LinkInput) -> u8 {
        if let Ok(u) = Url::parse(&input.raw) {
            if let Some(host) = u.host_str() {
                if Self::is_github_host(host) {
                    return 90; // 比 HTTP 更优先
                }
            }
        }
        0
    }

    async fn resolve(&self, input: &LinkInput, ctx: &ResolveContext) -> anyhow::Result<ResolveResult> {
        let u = Url::parse(&input.raw)?;

        // 1) blob -> raw
        let final_url = if let Some(raw) = Self::blob_to_raw(&u) {
            raw
        } else if u.host_str() == Some("github.com") {
            // 2) repo -> archive(main.zip)（可下载但不保证分支存在；失败可由HTTP重试/错误提示）
            if u.path_segments().map(|s| s.count()).unwrap_or(0) == 2 {
                Self::repo_archive(&u).unwrap_or(u.clone())
            } else {
                u.clone()
            }
        } else {
            // raw.githubusercontent.com or others
            u.clone()
        };

        let filename = final_url
            .path_segments()
            .and_then(|s| s.last())
            .filter(|s| !s.is_empty())
            .map(|s| sanitize(s))
            .unwrap_or_else(|| "github_download.bin".to_string());

        let suggested_path = ctx.out_dir.join(filename);

        let res = ResourceDescriptor {
            rtype: ResourceType::GitHubResolvedHttp,
            uri: final_url.to_string(),
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
