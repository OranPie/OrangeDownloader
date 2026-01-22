use anyhow::Context;
use tokio::fs::{File, OpenOptions};
use tokio::io::{AsyncSeekExt, AsyncWriteExt};
use std::path::Path;

pub struct Assembler {
    file: tokio::sync::Mutex<File>,
}

impl Assembler {
    pub async fn create(path: &Path, total_size: Option<u64>) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .write(true)
            .open(path)
            .await
            .with_context(|| format!("open {:?}", path))?;

        let assembler = Self { file: tokio::sync::Mutex::new(file) };

        if let Some(sz) = total_size {
            // 预分配/设置长度（稀疏文件方式）
            let f = assembler.file.lock().await;
            f.set_len(sz).await.ok(); // 某些平台/FS可能失败，忽略也可
        }

        Ok(assembler)
    }

    pub async fn write_at(&self, offset: u64, data: &[u8]) -> anyhow::Result<()> {
        let mut f = self.file.lock().await;
        f.seek(std::io::SeekFrom::Start(offset)).await?;
        f.write_all(data).await?;
        Ok(())
    }

    pub async fn flush(&self) -> anyhow::Result<()> {
        let mut f = self.file.lock().await;
        f.flush().await?;
        Ok(())
    }
}
