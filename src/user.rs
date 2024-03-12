use crate::{app_state::AppState, header::DbId, oauth::COOKIE_NAME};
use async_session::SessionStore;
use axum::TypedHeader;
use mysql_async::prelude::*;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::Mutex;

type Hss = HashSet<String>;

#[derive(Clone, Debug, Serialize)]
pub struct User {
    pub id: DbId,
    pub name: String,
    pub is_wiki_user: bool,
    pub auth_token: Option<String>,

    #[serde(skip_serializing)]
    app: Arc<AppState>,

    #[serde(skip_serializing)]
    access: Arc<Mutex<HashMap<DbId, Hss>>>,
}

impl User {
    pub async fn create_new(app: &Arc<AppState>, name: &str, is_wiki_user: bool) -> Option<Self> {
        let mut conn = app.get_gulp_conn().await.ok()?;
        let sql = "INSERT INTO `user` (`name`,`is_wiki_user`) VALUES (:name,:is_wiki_user)";
        conn.exec_drop(sql, params! {name, is_wiki_user})
            .await
            .ok()?;
        let user_id = conn.last_insert_id()?;
        drop(conn);
        Some(Self {
            id: user_id,
            name: name.to_string(),
            is_wiki_user,
            auth_token: None,
            app: app.clone(),
            access: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn get_or_create_wiki_user_id(app: &Arc<AppState>, username: &str) -> Option<DbId> {
        match Self::get_wiki_user_id(app, username).await {
            Some(ret) => Some(ret),
            None => {
                let sql = "INSERT IGNORE INTO `user` (`name`,`is_wiki_user`) VALUES (:username,1)";
                app.get_gulp_conn()
                    .await
                    .ok()?
                    .exec_drop(sql, params! {username})
                    .await
                    .ok()?;
                Self::get_wiki_user_id(app, username).await
            }
        }
    }

    async fn get_wiki_user_id(app: &Arc<AppState>, username: &str) -> Option<DbId> {
        let sql = "SELECT id FROM `user` WHERE `name`=:username AND is_wiki_user=1";
        app.get_gulp_conn()
            .await
            .ok()?
            .exec_iter(sql, params! {username})
            .await
            .ok()?
            .map_and_drop(mysql_async::from_row::<DbId>)
            .await
            .unwrap()
            .first()
            .cloned()
    }

    pub async fn from_cookies(
        app: &Arc<AppState>,
        cookies: &Option<TypedHeader<headers::Cookie>>,
        params: &HashMap<String, String>,
    ) -> Option<Self> {
        if let Some(user_id) = app.fixed_user_id {
            // Local testing only
            return Self::from_id(app, user_id).await;
        }
        if let Some(auth_token) = params.get("auth_token") {
            if !auth_token.is_empty() {
                return Self::from_auth_token(app, auth_token).await;
            }
        }
        let username = Self::get_user_name_from_cookies(app, cookies).await?;
        let user_id = Self::get_or_create_wiki_user_id(app, &username).await?;
        Self::from_id(app, user_id).await
    }

    async fn from_auth_token(app: &Arc<AppState>, auth_token: &str) -> Option<Self> {
        if auth_token.is_empty() {
            return None;
        }
        let sql =
            r#"SELECT id,name,is_wiki_user,access_token FROM `user` WHERE auth_token=:auth_token"#;
        let row = app
            .get_gulp_conn()
            .await
            .ok()?
            .exec_iter(sql, params! {auth_token})
            .await
            .ok()?
            .map_and_drop(|row| row)
            .await
            .ok()?
            .first()?
            .to_owned();
        Self::from_row(app, &row).await
    }

    async fn get_user_name_from_cookies(
        app: &Arc<AppState>,
        cookies: &Option<TypedHeader<headers::Cookie>>,
    ) -> Option<String> {
        let cookie = cookies.to_owned()?.get(COOKIE_NAME)?.to_string();
        let session = app.store.load_session(cookie).await.ok()??;
        let j = json!(session).get("data").cloned()?.get("user")?.to_owned();
        let user: Value = serde_json::from_str(j.as_str()?).ok()?;
        Some(user.get("username")?.as_str()?.to_string())
    }

    pub async fn from_id(app: &Arc<AppState>, user_id: DbId) -> Option<Self> {
        let sql = r#"SELECT id,name,is_wiki_user,access_token FROM `user` WHERE id=:user_id"#;
        let row = app
            .get_gulp_conn()
            .await
            .ok()?
            .exec_iter(sql, params! {user_id})
            .await
            .ok()?
            .map_and_drop(|row| row)
            .await
            .ok()?
            .first()?
            .to_owned();
        Self::from_row(app, &row).await
    }

    async fn from_row(app: &Arc<AppState>, row: &mysql_async::Row) -> Option<Self> {
        Some(Self {
            id: row.get(0)?,
            name: row.get(1)?,
            is_wiki_user: row.get(2)?,
            auth_token: row.get(3)?,
            app: app.clone(),
            access: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn get_access_for_list(&self, list_id: DbId) -> Hss {
        let mut access = self.access.lock().await;
        match access.get(&list_id) {
            Some(ret) => ret.to_owned(),
            None => {
                let user_id = self.id;
                // User #5 is "everyone that is logged in"
                let sql = "SELECT DISTINCT `right` FROM `access` WHERE `user_id` IN (5,:user_id) AND `list_id`=:list_id" ;
                let mut conn = match self.app.get_gulp_conn().await {
                    Ok(conn) => conn,
                    Err(_) => return Hss::new(),
                };
                let result = match conn.exec_iter(sql, params! {user_id,list_id}).await {
                    Ok(result) => result,
                    Err(_) => return Hss::new(),
                };
                let v = match result.map_and_drop(mysql_async::from_row::<String>).await {
                    Ok(v) => v,
                    Err(_) => return Hss::new(),
                };
                let ret: Hss = v.iter().cloned().collect();
                access.insert(list_id, ret.clone());
                ret
            }
        }
    }

    pub async fn can_create_new_data_source(&self, list_id: DbId) -> bool {
        let access = self.get_access_for_list(list_id).await;
        access.contains("admin")
            || access.contains("write")
            || access.contains("create_new_data_source")
    }

    pub async fn can_update_from_source(&self, list_id: DbId) -> bool {
        let access = self.get_access_for_list(list_id).await;
        access.contains("admin")
            || access.contains("write")
            || access.contains("update_from_source")
    }

    pub async fn can_create_snapshot(&self, list_id: DbId) -> bool {
        let access = self.get_access_for_list(list_id).await;
        access.contains("admin") || access.contains("write") || access.contains("create_snapshot")
    }

    pub async fn can_set_new_header_schema_for_list(&self, list_id: DbId) -> bool {
        let access = self.get_access_for_list(list_id).await;
        access.contains("admin")
            || access.contains("write")
            || access.contains("set_new_header_schema_for_list")
    }
    pub async fn can_edit_row(&self, list_id: DbId) -> bool {
        let access = self.get_access_for_list(list_id).await;
        access.contains("admin") || access.contains("write") || access.contains("edit_row")
    }
}
