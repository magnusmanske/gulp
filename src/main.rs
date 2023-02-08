use clap::{Parser, Subcommand};
use app_state::AppState;
use std::sync::Arc;
use api::run_server;

pub type GenericError = Box<dyn std::error::Error + Send + Sync>;

pub mod api;
pub mod app_state;
pub mod oauth;
pub mod database_session_store;
pub mod header;
pub mod cell;
pub mod row;
pub mod list;


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
async fn main() -> Result<(), GenericError> {
    let cli = Cli::parse();

    let app = Arc::new(AppState::from_config_file("config.json").expect("app creation failed"));

    match &cli.command {
        Some(Commands::Server) => {
            run_server(app).await?;
        }
        Some(Commands::Test) => {
            let list = AppState::get_list(&app,4).await.expect("List does not exists");
            let list = list.lock().await;
            //let revision_id = list.snapshot().await?;
            //println!("{revision_id:?}");
            //list.import_from_url("https://wikidata-todo.toolforge.org/file_candidates_hessen.txt",list::FileType::JSONL).await?;
            let rev0 = list.get_rows_for_revision(0).await?;
            let rev1 = list.get_rows_for_revision(1).await?;
            let rev1_sub: Vec<_> = rev1.iter().filter(|row|row.row_num==5075).collect();
            println!("{} / {} : {:#?}",rev0.len(),rev1.len(),rev1_sub);
        }
        None => {
            println!("Command required");
        }
    }


    Ok(())
}





/*
ssh magnus@tools-login.wmflabs.org -L 3308:tools-db:3306 -N &
*/