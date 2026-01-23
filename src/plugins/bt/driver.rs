use crate::plugins::registry::DriverContext;
use crate::core::model::ResourceDescriptor;
use anyhow::Context;
use librqbit::{AddTorrent, Session};
use std::path::Path;

pub struct BtDriver;

impl BtDriver {
    pub fn new() -> Self {
        Self
    }

    pub async fn download_magnet_to_dir(
        &self,
        res: &ResourceDescriptor,
        _ctx: &DriverContext,
        target_dir: &Path,
    ) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(target_dir).await?;

        let session = Session::new(target_dir.to_path_buf())
            .await
            .context("create bt session")?;

        let resp = session
            .add_torrent(AddTorrent::from_url(&res.uri), None)
            .await
            .context("add magnet")?;

        let handle = resp.into_handle().context("torrent handle")?;
        handle.wait_until_completed().await.context("bt wait complete")?;

        session.stop().await;
        Ok(())
    }
}
