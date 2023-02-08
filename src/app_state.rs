use std::{env, collections::HashMap};
use std::fs::File;
use std::time::Duration;
use mysql_async::{prelude::*,Conn,Opts,OptsBuilder,PoolConstraints,PoolOpts};
use oauth2::{AuthUrl, ClientId, ClientSecret, TokenUrl, RedirectUrl};
use oauth2::basic::BasicClient;
use serde_json::{Value, json};
use tokio::sync::{Mutex, RwLock};
use std::sync::Arc;
use crate::{list::List, header::DbId};
use crate::GenericError;
use crate::database_session_store::DatabaseSessionStore;

type ListMutex = Arc<Mutex<List>>;

#[derive(Debug, Clone)]
pub struct AppState {
    lists: Arc<RwLock<HashMap<DbId,ListMutex>>>,
    gulp_pool: mysql_async::Pool,
    _import_file_path: String,
    pub consumer_token: String,
    pub secret_token: String,
    pub store: DatabaseSessionStore,
    pub oauth_client: BasicClient,
}

impl AppState {
    /// Create an AppState object from a config JSION file
    pub fn from_config_file(filename: &str) -> Result<Self,GenericError> {
        let mut path = env::current_dir().expect("Can't get CWD");
        path.push(filename);
        let file = File::open(&path)?;
        let config: Value = serde_json::from_reader(file)?;
        Ok(Self::from_config(&config))
    }

    /// Creatre an AppState object from a config JSON object
    pub fn from_config(config: &Value) -> Self {
        let client_id = config["consumer_token"].as_str().unwrap().to_string();
        let client_secret = config["secret_token"].as_str().unwrap().to_string();
        let auth_url = "https://meta.wikimedia.org/w/rest.php/oauth2/authorize?response_type=code";
        let token_url = "https://meta.wikimedia.org/w/rest.php/oauth2/access_token";
        let redirect_url = "https://gulp.toolforge.org/auth/authorized" ;

        let oauth_client = BasicClient::new(
            ClientId::new(client_id),
            Some(ClientSecret::new(client_secret)),
            AuthUrl::new(auth_url.to_string()).unwrap(),
            Some(TokenUrl::new(token_url.to_string()).unwrap()),
        )
        .set_redirect_uri(RedirectUrl::new(redirect_url.to_string()).unwrap());

        let gulp_pool = Self::create_pool(&config["gulp"]);
        let ret = Self {
            lists: Arc::new(RwLock::new(HashMap::new())),
            gulp_pool: gulp_pool.clone(),
            // mnm_pool: Self::create_pool(&config["mixnmatch"]),
            _import_file_path: config["import_file_path"].as_str().unwrap().to_string(),
            consumer_token: config["consumer_token"].as_str().unwrap().to_string(),
            secret_token: config["secret_token"].as_str().unwrap().to_string(),
            store: DatabaseSessionStore{pool: Some(gulp_pool.clone())}, //MemoryStore::new(),//
            oauth_client,
        };
        ret
    }

    /// Helper function to create a DB pool from a JSON config object
    fn create_pool(config: &Value) -> mysql_async::Pool {
        let min_connections = config["min_connections"].as_u64().expect("No min_connections value") as usize;
        let max_connections = config["max_connections"].as_u64().expect("No max_connections value") as usize;
        let keep_sec = config["keep_sec"].as_u64().expect("No keep_sec value");
        let url = config["url"].as_str().expect("No url value");
        let pool_opts = PoolOpts::default()
            .with_constraints(PoolConstraints::new(min_connections, max_connections).unwrap())
            .with_inactive_connection_ttl(Duration::from_secs(keep_sec));
        let wd_url = url;
        let wd_opts = Opts::from_url(wd_url).expect(format!("Can not build options from db_wd URL {}",wd_url).as_str());
        mysql_async::Pool::new(OptsBuilder::from_opts(wd_opts).pool_opts(pool_opts.clone()))
    }

    /// Returns a connection to the GULP tool database
    pub async fn get_gulp_conn(&self) -> Result<Conn, mysql_async::Error> {
        self.gulp_pool.get_conn().await
    }

    async fn get_wiki_user_id(&self, username: &str) -> Option<DbId> {
        let sql = "SELECT id FROM `user` WHERE `name`=:username AND is_wiki_user=1" ;
        self.get_gulp_conn().await.ok()?
            .exec_iter(sql,params! {username}).await.ok()?
            .map_and_drop(|row| mysql_async::from_row::<DbId>(row)).await.unwrap().get(0).cloned()
    }
    
    pub async fn get_or_create_wiki_user_id(&self, username: &str) -> Option<DbId> {
        let res = self.get_wiki_user_id(username).await;
        if res.is_some() {
            return res;
        }
        let sql = "INSERT IGNORE INTO `user` (`name`,`is_wiki_user`) VALUES (:username,1)" ;
        self.get_gulp_conn().await.ok()?.exec_drop(sql, params!{username}).await.ok()?;
        let res = self.get_wiki_user_id(username).await;
        res
    }

    pub async fn get_lists_by_user_rights(&self, user_id: DbId, rights: &str) -> Option<Vec<Value>> {
        let rights: Vec<String> = rights.split(",").map(|s|s.trim().to_ascii_lowercase()).map(|s|s.replace("\"","")).filter(|s|!s.is_empty()).collect();
        let sql = if rights.is_empty() {
            format!(r#"SELECT list.id,list.name,list.revision_id,GROUP_CONCAT(DISTINCT `right`) FROM `list`,`access` WHERE user_id=:user_id AND list_id=list.id GROUP BY list.id"#)
        } else {
            let rights = format!("\"{}\"",rights.join("\",\""));
            format!(r#"SELECT list.id,list.name,list.revision_id,GROUP_CONCAT(DISTINCT `right`) FROM `list`,`access` WHERE user_id=:user_id AND list_id=list.id AND `right` IN ({rights}) GROUP BY list.id"#)
        };
        let rows = self.get_gulp_conn().await.ok()?
            .exec_iter(sql,params! {user_id}).await.ok()?
            .map_and_drop(|row| mysql_async::from_row::<(DbId,String,DbId,String)>(row)).await.ok()?;
        let lists: Vec<Value> = rows
            .iter()
            .map(|row|json!({"id":row.0,"name":row.1,"revision":row.2,"rights":row.3}))
            .collect();
        Some(lists)
    }

    pub async fn get_list(self: &Arc<AppState>, list_id: DbId) -> Option<Arc<Mutex<List>>> {
        if !self.lists.read().await.contains_key(&list_id) {
            let list = List::from_id(self, list_id).await?;
            self.lists.write().await.entry(list_id).or_insert(Arc::new(Mutex::new(list)));
        }
        self.lists.read().await.get(&list_id).map(|x|x.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_lists_by_user_rights() {
        let app = AppState::from_config_file("config.json").expect("app creation failed");
        let lists = app.get_lists_by_user_rights(1,"admin").await.unwrap();
        assert!(lists.iter().any(|list|list["id"]==4))
    }
}
