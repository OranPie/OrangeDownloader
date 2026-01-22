use async_trait::async_trait;
use crate::plugins::registry::{LinkResolver, ResolveContext, ResolveResult};
use crate::core::model::LinkInput;

pub struct Ed2kResolver;
impl Ed2kResolver { pub fn new() -> Self { Self } }

#[async_trait]
impl LinkResolver for Ed2kResolver {
    fn name(&self) -> &'static str { "ed2k-resolver-stub" }

    fn can_handle(&self, input: &LinkInput) -> u8 {
        if input.raw.to_ascii_lowercase().starts_with("ed2k://") { 80 } else { 0 }
    }

    async fn resolve(&self, _input: &LinkInput, _ctx: &ResolveContext) -> anyhow::Result<ResolveResult> {
        anyhow::bail!("ED2K plugin stub: not implemented yet (session-based).");
    }
}
    