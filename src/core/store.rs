use crate::core::model::FragmentState;
use anyhow::Context;
use sqlx::{Row, SqlitePool};
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct SqliteStore {
    pool: SqlitePool,
}

#[derive(Debug, Clone)]
pub struct ItemRecord {
    pub item_db_id: i64,
    pub downloaded_bytes: i64,
    pub total_size: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct FragmentRecord {
    pub frag_db_id: i64,
    pub offset: i64,
    pub len: i64,
    pub state: FragmentState,
}

impl SqliteStore {
    pub async fn open(db_path: &Path) -> anyhow::Result<Self> {
        use anyhow::Context;

        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await
                .with_context(|| format!("create_dir_all {}", parent.display()))?;
        }

        let abs = if db_path.is_absolute() {
            db_path.to_path_buf()
        } else {
            std::env::current_dir()
                .with_context(|| "current_dir")?
                .join(db_path)
        };

        let mut p = abs.to_string_lossy().to_string();
        if cfg!(windows) {
            p = p.replace('\\', "/");
        }

        // ✅ 关键：加 mode=rwc 允许不存在时创建
        let url = if p.starts_with('/') {
            // Unix absolute => sqlite:////Users/.../file.sqlite?mode=rwc
            format!("sqlite://{}?mode=rwc", p)
        } else {
            // Windows => sqlite:///C:/.../file.sqlite?mode=rwc
            format!("sqlite:///{}?mode=rwc", p)
        };

        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(5)
            .connect(&url)
            .await
            .with_context(|| format!("connect sqlite url={} (file={})", url, abs.display()))?;

        let store = Self { pool };
        store.migrate().await?;
        Ok(store)
    }



    async fn migrate(&self) -> anyhow::Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS items (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              source_uri TEXT NOT NULL,
              target_path TEXT NOT NULL,
              partial_path TEXT NOT NULL,
              total_size INTEGER NULL,
              chunk_size INTEGER NOT NULL,
              supports_ranges INTEGER NOT NULL,
              downloaded_bytes INTEGER NOT NULL DEFAULT 0,
              updated_at INTEGER NOT NULL
            );
            "#,
        )
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE UNIQUE INDEX IF NOT EXISTS idx_items_unique
            ON items(source_uri, target_path);
            "#,
        )
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS fragments (
              id INTEGER PRIMARY KEY AUTOINCREMENT,
              item_id INTEGER NOT NULL,
              offset INTEGER NOT NULL,
              len INTEGER NOT NULL,
              state INTEGER NOT NULL, -- 0 Missing,1 Downloading,2 Done,3 Bad
              updated_at INTEGER NOT NULL,
              FOREIGN KEY(item_id) REFERENCES items(id)
            );
            "#,
        )
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_frag_item
            ON fragments(item_id);
            "#,
        )
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    fn now_epoch() -> i64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    pub async fn upsert_item(
        &self,
        source_uri: &str,
        target_path: &Path,
        partial_path: &Path,
        chunk_size: i64,
        total_size: Option<i64>,
        supports_ranges: bool,
    ) -> anyhow::Result<ItemRecord> {
        let now = Self::now_epoch();
        let supports_ranges_i = if supports_ranges { 1 } else { 0 };

        sqlx::query(
            r#"
            INSERT OR IGNORE INTO items
              (source_uri, target_path, partial_path, total_size, chunk_size, supports_ranges, downloaded_bytes, updated_at)
            VALUES
              (?, ?, ?, ?, ?, ?, 0, ?);
            "#,
        )
            .bind(source_uri)
            .bind(target_path.to_string_lossy().to_string())
            .bind(partial_path.to_string_lossy().to_string())
            .bind(total_size)
            .bind(chunk_size)
            .bind(supports_ranges_i)
            .bind(now)
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            UPDATE items
            SET total_size = COALESCE(?, total_size),
                chunk_size = ?,
                supports_ranges = ?,
                updated_at = ?
            WHERE source_uri = ? AND target_path = ?;
            "#,
        )
            .bind(total_size)
            .bind(chunk_size)
            .bind(supports_ranges_i)
            .bind(now)
            .bind(source_uri)
            .bind(target_path.to_string_lossy().to_string())
            .execute(&self.pool)
            .await?;

        self.get_item(source_uri, target_path).await
    }

    pub async fn get_item(&self, source_uri: &str, target_path: &Path) -> anyhow::Result<ItemRecord> {
        let row = sqlx::query(
            r#"
            SELECT id, downloaded_bytes, total_size
            FROM items
            WHERE source_uri = ? AND target_path = ?;
            "#,
        )
            .bind(source_uri)
            .bind(target_path.to_string_lossy().to_string())
            .fetch_one(&self.pool)
            .await
            .context("fetch item")?;

        Ok(ItemRecord {
            item_db_id: row.get::<i64, _>("id"),
            downloaded_bytes: row.get::<i64, _>("downloaded_bytes"),
            total_size: row.try_get::<i64, _>("total_size").ok(),
        })
    }

    pub async fn load_fragments(&self, item_db_id: i64) -> anyhow::Result<Vec<FragmentRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, offset, len, state
            FROM fragments
            WHERE item_id = ?
            ORDER BY offset ASC;
            "#,
        )
            .bind(item_db_id)
            .fetch_all(&self.pool)
            .await?;

        Ok(rows
            .into_iter()
            .map(|r| FragmentRecord {
                frag_db_id: r.get::<i64, _>("id"),
                offset: r.get::<i64, _>("offset"),
                len: r.get::<i64, _>("len"),
                state: int_to_state(r.get::<i64, _>("state")),
            })
            .collect())
    }

    pub async fn ensure_fragments_for_ranges(
        &self,
        item_db_id: i64,
        ranges: &[(u64, u64)], // (offset,len)
    ) -> anyhow::Result<()> {
        let row = sqlx::query(r#"SELECT COUNT(1) as cnt FROM fragments WHERE item_id = ?"#)
            .bind(item_db_id)
            .fetch_one(&self.pool)
            .await?;
        let existing: i64 = row.get::<i64, _>("cnt");
        if existing > 0 {
            return Ok(());
        }

        let now = Self::now_epoch();
        for (offset, len) in ranges {
            sqlx::query(
                r#"
                INSERT INTO fragments(item_id, offset, len, state, updated_at)
                VALUES(?, ?, ?, ?, ?);
                "#,
            )
                .bind(item_db_id)
                .bind(*offset as i64)
                .bind(*len as i64)
                .bind(state_to_int(FragmentState::Missing))
                .bind(now)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }

    pub async fn mark_fragment_done_and_add_bytes(
        &self,
        frag_db_id: i64,
        item_db_id: i64,
        bytes: i64,
    ) -> anyhow::Result<()> {
        let now = Self::now_epoch();

        sqlx::query(
            r#"
            UPDATE fragments
            SET state = ?, updated_at = ?
            WHERE id = ?;
            "#,
        )
            .bind(state_to_int(FragmentState::Done))
            .bind(now)
            .bind(frag_db_id)
            .execute(&self.pool)
            .await?;

        sqlx::query(
            r#"
            UPDATE items
            SET downloaded_bytes = downloaded_bytes + ?,
                updated_at = ?
            WHERE id = ?;
            "#,
        )
            .bind(bytes)
            .bind(now)
            .bind(item_db_id)
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    pub async fn set_fragment_state(&self, frag_db_id: i64, state: FragmentState) -> anyhow::Result<()> {
        let now = Self::now_epoch();
        sqlx::query(
            r#"
            UPDATE fragments
            SET state = ?, updated_at = ?
            WHERE id = ?;
            "#,
        )
            .bind(state_to_int(state))
            .bind(now)
            .bind(frag_db_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn read_partial_path(&self, source_uri: &str, target_path: &Path) -> anyhow::Result<PathBuf> {
        let row = sqlx::query(
            r#"
            SELECT partial_path FROM items
            WHERE source_uri = ? AND target_path = ?;
            "#,
        )
            .bind(source_uri)
            .bind(target_path.to_string_lossy().to_string())
            .fetch_one(&self.pool)
            .await?;
        Ok(PathBuf::from(row.get::<String, _>("partial_path")))
    }

    pub async fn set_item_supports_ranges(&self, item_db_id: i64, supports_ranges: bool) -> anyhow::Result<()> {
        let now = Self::now_epoch();
        let v = if supports_ranges { 1 } else { 0 };
        sqlx::query(
            r#"UPDATE items SET supports_ranges = ?, updated_at = ? WHERE id = ?"#,
        )
            .bind(v)
            .bind(now)
            .bind(item_db_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_fragments(&self, item_db_id: i64) -> anyhow::Result<()> {
        sqlx::query(r#"DELETE FROM fragments WHERE item_id = ?"#)
            .bind(item_db_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

}

fn state_to_int(s: FragmentState) -> i64 {
    match s {
        FragmentState::Missing => 0,
        FragmentState::Downloading => 1,
        FragmentState::Done => 2,
        FragmentState::Bad => 3,
    }
}

fn int_to_state(v: i64) -> FragmentState {
    match v {
        0 => FragmentState::Missing,
        1 => FragmentState::Downloading,
        2 => FragmentState::Done,
        3 => FragmentState::Bad,
        _ => FragmentState::Missing,
    }
}
