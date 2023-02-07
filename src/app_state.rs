use std::{env, collections::HashMap};
use std::fs::File;
use std::time::Duration;

use mysql_async::{Conn,Opts,OptsBuilder,PoolConstraints,PoolOpts};
use serde_json::Value;
use crate::{list::List, header::DbId};
use tokio::sync::{Mutex, RwLock};
use std::sync::Arc;
// use configuration::Configuration;
// use mysql_async::from_row;
// use mysql_async::prelude::*;

use crate::GenericError;

type ListMutex = Arc<Mutex<List>>;

#[derive(Debug, Clone)]
pub struct AppState {
    lists: Arc<RwLock<HashMap<DbId,ListMutex>>>,
    gulp_pool: mysql_async::Pool,
    _import_file_path: String,
    _bot_name: String,
    _bot_password: String,
    consumer_token: String,
    secret_token: String,
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
        let ret = Self {
            lists: Arc::new(RwLock::new(HashMap::new())),
            gulp_pool: Self::create_pool(&config["gulp"]),
            // mnm_pool: Self::create_pool(&config["mixnmatch"]),
            _import_file_path: config["import_file_path"].as_str().unwrap().to_string(),
            _bot_name: config["bot_name"].as_str().unwrap().to_string(),
            _bot_password: config["bot_password"].as_str().unwrap().to_string(),
            consumer_token: config["consumer_token"].as_str().unwrap().to_string(),
            secret_token: config["secret_token"].as_str().unwrap().to_string(),
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

    pub async fn get_list(self: &Arc<AppState>, list_id: DbId) -> Option<Arc<Mutex<List>>> {
        if !self.lists.read().await.contains_key(&list_id) {
            let list = List::from_id(self, list_id).await?;
            self.lists.write().await.entry(list_id).or_insert(Arc::new(Mutex::new(list)));
        }
        self.lists.read().await.get(&list_id).map(|x|x.clone())
    }
}