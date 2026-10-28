//! Persistent destinations for successfully uploaded documents. Library identity
//! excludes credentials, and changes when the configured server or user changes.

use super::{ZoteroClient, ZoteroMode, LOCAL_BASE, LOCAL_USER_ID};
use crate::error::Result;
use sqlx::SqlitePool;
use std::collections::HashMap;

impl ZoteroMode {
    pub fn library_id(&self) -> String {
        match self {
            Self::Cloud { user_id, .. } => format!("cloud:{}", user_id.trim()),
            Self::Local {
                base_url,
                user_id_override,
                ..
            } => {
                let base = base_url
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(LOCAL_BASE)
                    .trim_end_matches('/');
                let user = user_id_override
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(LOCAL_USER_ID);
                format!("local:{}:users/{}", base, user)
            }
        }
    }

    pub(super) fn document_url(&self, attachment_key: &str, mime: Option<&str>) -> String {
        match self {
            Self::Cloud { user_id, .. } => format!(
                "https://www.zotero.org/users/{}/items/{}",
                user_id.trim(),
                attachment_key,
            ),
            Self::Local { .. } => {
                let action = if mime.map(|m| m.split(';').next().unwrap_or("").trim())
                    == Some("application/pdf")
                {
                    "open-pdf"
                } else {
                    "select"
                };
                format!("zotero://{}/library/items/{}", action, attachment_key)
            }
        }
    }
}

impl ZoteroClient {
    /// A webpage is itself the destination; it has no file attachment key.
    pub async fn remember_web_link(
        &self, pool: &SqlitePool, course_id: &str, topic_id: &str, item_key: &str,
    ) -> Result<()> {
        self.remember_document(pool, course_id, topic_id, item_key, None).await
    }

    /// Call only after upload/replacement succeeds, or an existing attachment
    /// is confirmed current. A failed upload must never create a redirect.
    pub async fn remember_document(
        &self,
        pool: &SqlitePool,
        course_id: &str,
        source_id: &str,
        attachment_key: &str,
        mime: Option<&str>,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO zotero_document_links (library_id, course_id, source_id, url)
             VALUES (?, ?, ?, ?)
             ON CONFLICT(library_id, course_id, source_id) DO UPDATE SET url = excluded.url",
        )
        .bind(self.mode.library_id())
        .bind(course_id)
        .bind(source_id)
        .bind(self.mode.document_url(attachment_key, mime))
        .execute(pool)
        .await?;
        Ok(())
    }
}

pub async fn load(
    pool: &SqlitePool,
    mode: &ZoteroMode,
    course_id: &str,
) -> Result<HashMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT source_id, url FROM zotero_document_links WHERE library_id = ? AND course_id = ?",
    )
    .bind(mode.library_id())
    .bind(course_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().collect())
}

/// Restrict custom-protocol launching to document destinations we generate.
pub fn is_document_url(url: &str) -> bool {
    [
        "zotero://open-pdf/library/items/",
        "zotero://select/library/items/",
    ]
    .iter()
    .any(|prefix| {
        url.strip_prefix(prefix).is_some_and(|key| {
            key.len() == 8
                && key
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local() -> ZoteroMode {
        ZoteroMode::Local {
            base_url: None,
            basic_auth: None,
            user_id_override: None,
            api_key: None,
        }
    }

    #[test]
    fn destinations_open_the_attachment_in_the_configured_library() {
        assert_eq!(
            local().document_url("ABCD1234", Some("application/pdf; charset=binary")),
            "zotero://open-pdf/library/items/ABCD1234"
        );
        assert_eq!(
            local().document_url("ABCD1234", Some("application/msword")),
            "zotero://select/library/items/ABCD1234"
        );
        let cloud = ZoteroMode::Cloud {
            user_id: " 42 ".into(),
            api_key: "secret".into(),
        };
        assert_eq!(
            cloud.document_url("ABCD1234", None),
            "https://www.zotero.org/users/42/items/ABCD1234"
        );
        assert!(!cloud.library_id().contains("secret"));
    }

    #[test]
    fn custom_protocol_validation_rejects_other_actions_and_arguments() {
        assert!(is_document_url("zotero://open-pdf/library/items/ABCD1234"));
        assert!(is_document_url("zotero://select/library/items/ABCD1234"));
        for url in [
            "zotero://delete/library/items/ABCD1234",
            "file:///tmp/document",
            "zotero://select/library/items/ABCD1234?other=1",
            "zotero://select/library/items/../../x",
        ] {
            assert!(!is_document_url(url), "{}", url);
        }
    }

    #[tokio::test]
    async fn saved_links_are_scoped_to_course_and_library_and_can_be_replaced() {
        let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::raw_sql(include_str!(
            "../../migrations/0025_zotero_document_links.sql"
        ))
        .execute(&pool)
        .await
        .unwrap();
        let zc = ZoteroClient::new(local()).await.unwrap();
        zc.remember_document(
            &pool,
            "course1",
            "topic1",
            "ABCD1234",
            Some("application/pdf"),
        )
        .await
        .unwrap();
        zc.remember_document(&pool, "course2", "topic1", "EFGH5678", None)
            .await
            .unwrap();
        assert_eq!(
            load(&pool, &local(), "course1").await.unwrap()["topic1"],
            "zotero://open-pdf/library/items/ABCD1234"
        );
        let cloud = ZoteroMode::Cloud {
            user_id: "42".into(),
            api_key: "key".into(),
        };
        assert!(load(&pool, &cloud, "course1").await.unwrap().is_empty());
        zc.remember_document(&pool, "course1", "topic1", "IJKL9012", None)
            .await
            .unwrap();
        assert_eq!(load(&pool, &local(), "course1").await.unwrap().len(), 1);
        assert_eq!(
            load(&pool, &local(), "course1").await.unwrap()["topic1"],
            "zotero://select/library/items/IJKL9012"
        );
    }
}
