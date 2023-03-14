use std::sync::Arc;
use async_session::SessionStore;
use axum::TypedHeader;
use mysql_async::prelude::*;
use serde::{Serialize, Deserialize};
use serde_json::{json, Value};
use crate::{header::DbId, app_state::AppState, oauth::COOKIE_NAME};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: DbId,
    pub name: String,
    pub is_wiki_user: bool,
}

impl User {
    pub async fn create_new(app: &Arc<AppState>, name: &str, is_wiki_user: bool) -> Option<Self> {
        let mut conn = app.get_gulp_conn().await.ok()?;
        let sql = "INSERT INTO `file` (`name`,`is_wiki_user`) VALUES (:name,:is_wiki_user)" ;
        conn.exec_drop(sql, params!{name, is_wiki_user}).await.ok()?;
        let user_id = conn.last_insert_id()?;
        drop(conn);
        Some(Self{ id: user_id, name: name.to_string(), is_wiki_user })
    }

    pub async fn get_or_create_wiki_user_id(app: &Arc<AppState>, username: &str) -> Option<DbId> {
        match Self::get_wiki_user_id(app, username).await {
            Some(ret) => Some(ret),
            None => {
                let sql = "INSERT IGNORE INTO `user` (`name`,`is_wiki_user`) VALUES (:username,1)" ;
                app.get_gulp_conn().await.ok()?.exec_drop(sql, params!{username}).await.ok()?;
                Self::get_wiki_user_id(app, username).await        
            }
        }
    }

    async fn get_wiki_user_id(app: &Arc<AppState>, username: &str) -> Option<DbId> {
        let sql = "SELECT id FROM `user` WHERE `name`=:username AND is_wiki_user=1" ;
        app.get_gulp_conn().await.ok()?
            .exec_iter(sql,params! {username}).await.ok()?
            .map_and_drop(|row| mysql_async::from_row::<DbId>(row)).await.unwrap().get(0).cloned()
    }
    

    pub async fn from_cookies(app: &Arc<AppState>, cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<Self> {
        let username = Self::get_user_name_from_cookies(app, cookies).await?;
        let user_id = Self::get_or_create_wiki_user_id(&app, &username).await?;
        Self::from_id(app, user_id).await
    }

    async fn get_user_name_from_cookies(app: &Arc<AppState>, cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<String> {
        let cookie = cookies.to_owned()?.get(COOKIE_NAME)?.to_string();
        let session = app.store.load_session(cookie).await.ok()??;
        let j = json!(session).get("data").cloned()?.get("user")?.to_owned();
        let user: Value = serde_json::from_str(j.as_str()?).ok()?;
        Some(user.get("username")?.as_str()?.to_string())
    }

    pub async fn from_id(app: &Arc<AppState>, user_id: DbId) -> Option<Self> {
        let sql = r#"SELECT id,name,is_wiki_user FROM `file` WHERE id=:user_id"#;
        let row = app.get_gulp_conn().await.ok()?
            .exec_iter(sql,params! {user_id}).await.ok()?
            .map_and_drop(|row| row).await.ok()?
            .get(0)?.to_owned();
        Self::from_row(&row).await
    }

    async fn from_row(row: &mysql_async::Row) -> Option<Self> {
        Some(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            is_wiki_user: row.get(2)?,
        })
    }

}