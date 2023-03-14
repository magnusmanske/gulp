use std::sync::Arc;
use mysql_async::prelude::*;
use serde::{Serialize, Deserialize};
use crate::{header::DbId, app_state::AppState};



#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct File {
    pub id: DbId,
    pub path: String,
    pub user_id: DbId,
}

impl File {
    pub async fn create_new(app: &Arc<AppState>, path: &str, user_id: DbId) -> Option<Self> {
        let mut conn = app.get_gulp_conn().await.ok()?;
        let sql = "INSERT INTO `file` (`path`,`user_id`) VALUES (:path,:user_id)" ;
        conn.exec_drop(sql, params!{path, user_id}).await.ok()?;
        let file_id = conn.last_insert_id()?;
        drop(conn);
        Some(Self{ id: file_id, path: path.to_string(), user_id })
    }

    pub async fn from_id(app: &Arc<AppState>, file_id: DbId) -> Option<Self> {
        let sql = r#"SELECT id,path,user_id FROM `file` WHERE id=:file_id"#;
        let row = app.get_gulp_conn().await.ok()?
            .exec_iter(sql,params! {file_id}).await.ok()?
            .map_and_drop(|row| row).await.ok()?
            .get(0)?.to_owned();
        Self::from_row(&row).await
    }

    async fn from_row(row: &mysql_async::Row) -> Option<Self> {
        Some(Self {
            id: row.get(0)?,
            path: row.get(1)?,
            user_id: row.get(2)?,
        })
    }

}