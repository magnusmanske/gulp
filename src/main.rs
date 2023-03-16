#[macro_use]
extern crate lazy_static;

use clap::{Parser, Subcommand};
use app_state::AppState;
use header::HeaderSchema;
use std::sync::Arc;
use api::run_server;
pub use error::GulpError;

pub mod error;
pub mod api;
pub mod app_state;
pub mod oauth;
pub mod database_session_store;
pub mod data_source;
pub mod gulp_response;
pub mod header;
pub mod cell;
pub mod row;
pub mod list;
pub mod file;
pub mod user;


#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Server,
    Test,
}



#[tokio::main]
async fn main() -> Result<(), GulpError> {
    let cli = Cli::parse();

    let app = Arc::new(AppState::from_config_file("config.json").expect("app creation failed"));

    match &cli.command {
        Some(Commands::Server) => {
            run_server(app).await?;
        }
        Some(Commands::Test) => {
            let hs = HeaderSchema::from_id_app(&app,6).await.unwrap();
            println!("{}",hs.generate_name());
            // let source = DataSource::from_db(&app,8).await.unwrap();
            // let list = list::List::from_id(&app, source.list_id).await.unwrap();
            // list.update_from_source(&source, 1).await.unwrap();

            // let session = app.store.load_session("yFE28eun2Mqag9y/g9+PqG2zeULtmLlCs3+C9ExmJiw=".to_string()).await;
            // let session = session.unwrap().unwrap();
            // let j = json!(session).get("data").cloned().unwrap();
            // let j = j.get("user").unwrap().as_str().unwrap();
            // let user: serde_json::Value = serde_json::from_str(j).unwrap();
            // let username = user.get("username").unwrap().as_str().unwrap();
            // let user_id = app.get_or_create_wiki_user_id(username).await.unwrap();

            //let cookie = "yFE28eun2Mqag9y/g9+PqG2zeULtmLlCs3+C9ExmJiw=".to_string();
            //let session = app.load_session(cookie).await.ok();

            // let j = r#"{"data":{"user":"{\"username\":\"Magnus Manske\",\"realname\":\"\",\"email\":\"magnusmanske@googlemail.com\",\"editcount\":1299,\"confirmed_email\":true,\"blocked\":false,\"groups\":[\"oauthadmin\",\"*\",\"user\",\"autoconfirmed\"],\"rights\":[],\"grants\":[\"mwoauth-authonlyprivate\"]}"},"expiry":null,"id":"yFE28eun2Mqag9y/g9+PqG2zeULtmLlCs3+C9ExmJiw="}"#;
            // let j: serde_json::Value = serde_json::from_str(&j).unwrap();
            // let j = json!(j).get("data").cloned().unwrap().get("user").unwrap().to_owned();
            // let j: serde_json::Value = serde_json::from_str(j.as_str().unwrap()).unwrap();
            // println!("{j:?}");
            // let user_name = j.get("username").unwrap().as_str().unwrap();
            // let user_id = app.get_or_create_wiki_user_id(user_name).await.unwrap();
            // println!("{user_id}");
        
            /*
            let list = AppState::get_list(&app,4).await.expect("List does not exists");
            let list = list.lock().await;
            //let revision_id = list.snapshot().await?;
            //println!("{revision_id:?}");
            //list.import_from_url("https://wikidata-todo.toolforge.org/file_candidates_hessen.txt",list::DataSourceFormat::JSONL).await?;
            let rev0 = list.get_rows_for_revision(0).await?;
            let rev1 = list.get_rows_for_revision(1).await?;
            let rev1_sub: Vec<_> = rev1.iter().filter(|row|row.row_num==5075).collect();
            println!("{} / {} : {:#?}",rev0.len(),rev1.len(),rev1_sub);
            */
        }
        None => {
            println!("Command required");
        }
    }


    Ok(())
}





/*
ssh magnus@tools-login.wmflabs.org -L 3308:tools-db:3306 -N &
ssh magnus@tools-login.wmflabs.org -L 3309:wikidatawiki.analytics.db.svc.wikimedia.cloud:3306 -N &
*/