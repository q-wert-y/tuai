//! SQLite 实现（rusqlite bundled，首次运行自动建库）。

use crate::model::{Message, Role, Session};
use crate::util::now_secs;
use anyhow::Context;
use rusqlite::{params, Connection};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    title         TEXT NOT NULL,
    provider      TEXT NOT NULL,
    model         TEXT NOT NULL,
    system_prompt TEXT,
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS messages (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id INTEGER NOT NULL,
    role       TEXT NOT NULL,
    content    TEXT NOT NULL,
    reasoning  TEXT,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);
"#;

/// 数据库句柄。同步短操作，仅在主循环线程使用。
pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(data_dir: &std::path::Path) -> anyhow::Result<Store> {
        let path = data_dir.join("tuai.db");
        let conn = Connection::open(&path)
            .with_context(|| format!("打开数据库失败: {}", path.display()))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn })
    }

    fn row_to_session(row: &rusqlite::Row) -> rusqlite::Result<Session> {
        Ok(Session {
            id: row.get(0)?,
            title: row.get(1)?,
            provider: row.get(2)?,
            model: row.get(3)?,
            system_prompt: row.get(4)?,
            created_at: row.get(5)?,
            updated_at: row.get(6)?,
        })
    }

    fn row_to_message(row: &rusqlite::Row) -> rusqlite::Result<Message> {
        Ok(Message {
            id: row.get(0)?,
            session_id: row.get(1)?,
            role: Role::parse(&row.get::<_, String>(2)?).unwrap_or(Role::User),
            content: row.get(3)?,
            reasoning: row.get(4)?,
            created_at: row.get(5)?,
        })
    }

    /// 列出全部会话（按最近更新排序）。
    pub fn list_sessions(&self) -> anyhow::Result<Vec<Session>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, title, provider, model, system_prompt, created_at, updated_at FROM sessions ORDER BY updated_at DESC, id DESC")?;
        let rows = stmt
            .query_map([], Self::row_to_session)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 新建会话。
    #[allow(clippy::too_many_arguments)]
    pub fn create_session(
        &self,
        title: &str,
        provider: &str,
        model: &str,
        system_prompt: Option<&str>,
    ) -> anyhow::Result<Session> {
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO sessions (title, provider, model, system_prompt, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
            params![title, provider, model, system_prompt, now],
        )?;
        let id = self.conn.last_insert_rowid();
        Ok(Session {
            id,
            title: title.to_string(),
            provider: provider.to_string(),
            model: model.to_string(),
            system_prompt: system_prompt.map(|s| s.to_string()),
            created_at: now,
            updated_at: now,
        })
    }

    pub fn update_session_title(&self, id: i64, title: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET title = ?1, updated_at = ?2 WHERE id = ?3",
            params![title, now_secs(), id],
        )?;
        Ok(())
    }

    pub fn update_session_model(&self, id: i64, model: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET model = ?1 WHERE id = ?2",
            params![model, id],
        )?;
        Ok(())
    }

    pub fn update_session_provider(&self, id: i64, provider: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET provider = ?1 WHERE id = ?2",
            params![provider, id],
        )?;
        Ok(())
    }

    pub fn update_session_system_prompt(
        &self,
        id: i64,
        prompt: Option<&str>,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET system_prompt = ?1 WHERE id = ?2",
            params![prompt, id],
        )?;
        Ok(())
    }

    /// 提供商改名时同步会话引用。
    pub fn rename_session_provider(&self, old: &str, new: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET provider = ?1 WHERE provider = ?2",
            params![new, old],
        )?;
        Ok(())
    }

    pub fn delete_session(&self, id: i64) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
        self.conn
            .execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn touch_session(&self, id: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sessions SET updated_at = ?1 WHERE id = ?2",
            params![now_secs(), id],
        )?;
        Ok(())
    }

    /// 某会话的全部消息（按时间正序）。
    pub fn messages(&self, session_id: i64) -> anyhow::Result<Vec<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, session_id, role, content, reasoning, created_at FROM messages WHERE session_id = ?1 ORDER BY id ASC",
        )?;
        let rows = stmt
            .query_map(params![session_id], Self::row_to_message)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    /// 插入消息（每条完成后立即落盘）。
    pub fn insert_message(
        &self,
        session_id: i64,
        role: Role,
        content: &str,
        reasoning: Option<&str>,
    ) -> anyhow::Result<Message> {
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO messages (session_id, role, content, reasoning, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![session_id, role.as_str(), content, reasoning, now],
        )?;
        let id = self.conn.last_insert_rowid();
        self.touch_session(session_id)?;
        Ok(Message {
            id,
            session_id,
            role,
            content: content.to_string(),
            reasoning: reasoning.filter(|r| !r.is_empty()).map(|r| r.to_string()),
            created_at: now,
        })
    }

    /// 删除某消息之后（不含）的所有消息（用于重新生成）。
    pub fn delete_messages_after(&self, session_id: i64, message_id: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM messages WHERE session_id = ?1 AND id > ?2",
            params![session_id, message_id],
        )?;
        Ok(())
    }

    /// 更新消息内容（编辑消息）。
    pub fn update_message_content(&self, id: i64, content: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE messages SET content = ?1 WHERE id = ?2",
            params![content, id],
        )?;
        Ok(())
    }

    /// 删除单条消息。
    pub fn delete_message(&self, id: i64) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 清空会话消息。
    pub fn clear_messages(&self, session_id: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM messages WHERE session_id = ?1",
            params![session_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_store() -> Store {
        // 每个测试独立目录（并行测试互不干扰）
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("tuai-test-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Store::open(&dir).unwrap()
    }

    #[test]
    fn session_roundtrip() {
        let s = tmp_store();
        let sess = s
            .create_session("标题", "deepseek", "deepseek-chat", Some("你是助手"))
            .unwrap();
        assert_eq!(sess.title, "标题");
        let list = s.list_sessions().unwrap();
        assert_eq!(list.len(), 1);
        s.update_session_title(sess.id, "新标题").unwrap();
        assert_eq!(s.list_sessions().unwrap()[0].title, "新标题");
    }

    #[test]
    fn message_roundtrip() {
        let s = tmp_store();
        let sess = s.create_session("t", "p", "m", None).unwrap();
        let m1 = s.insert_message(sess.id, Role::User, "你好", None).unwrap();
        let m2 = s
            .insert_message(sess.id, Role::Assistant, "你好！", Some("思考"))
            .unwrap();
        let msgs = s.messages(sess.id).unwrap();
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].content, "你好");
        assert_eq!(msgs[1].reasoning.as_deref(), Some("思考"));
        // 删除 m1 之后的消息 → 只剩 m1
        s.delete_messages_after(sess.id, m1.id).unwrap();
        assert_eq!(s.messages(sess.id).unwrap().len(), 1);
        // 清空
        s.clear_messages(sess.id).unwrap();
        assert!(s.messages(sess.id).unwrap().is_empty());
        assert_eq!(m2.role, Role::Assistant);
    }

    #[test]
    fn message_edit_delete() {
        let s = tmp_store();
        let sess = s.create_session("t", "p", "m", None).unwrap();
        let m1 = s.insert_message(sess.id, Role::User, "旧", None).unwrap();
        s.insert_message(sess.id, Role::Assistant, "答", None)
            .unwrap();
        s.update_message_content(m1.id, "新内容").unwrap();
        assert_eq!(s.messages(sess.id).unwrap()[0].content, "新内容");
        s.delete_message(m1.id).unwrap();
        let rest = s.messages(sess.id).unwrap();
        assert_eq!(rest.len(), 1);
        assert_eq!(rest[0].role, Role::Assistant);
    }

    #[test]
    fn delete_session_cascade() {
        let s = tmp_store();
        let sess = s.create_session("t", "p", "m", None).unwrap();
        s.insert_message(sess.id, Role::User, "x", None).unwrap();
        s.delete_session(sess.id).unwrap();
        assert!(s.list_sessions().unwrap().is_empty());
        assert!(s.messages(sess.id).unwrap().is_empty());
    }
}
