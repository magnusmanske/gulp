use clap::{Parser, Subcommand};
use app_state::AppState;
use oauth::*;
use axum::{
    routing::get,
    Json, 
    Router,
    extract::Path,
    http::StatusCode,
};
use header::DbId;
use serde_json::json;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing;
use tracing_subscriber;
use tower_http::cors::{Any, CorsLayer};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use axum_extra::routing::SpaRouter;
use async_session::SessionStore;
use axum::{
    Server,
    extract::State,
    extract::Query,
    extract::{
        TypedHeader,
    },
    response::{IntoResponse, Response},
};




pub type GenericError = Box<dyn std::error::Error + Send + Sync>;

pub mod app_state;
pub mod oauth;
pub mod database_session_store;
pub mod header;
pub mod cell;
pub mod row;
pub mod list;

async fn get_user(state: &Arc<AppState>,cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<serde_json::Value> {
    let cookies = match cookies {
        Some(cookies) => cookies,
        None => return None,
    };
    let cookie = cookies.get(COOKIE_NAME).unwrap();
    match state.store.load_session(cookie.to_string()).await.unwrap() {
        Some(session) => {
            let j = json!(session);
            j.get("data").cloned()
        }
        None => None
    }
}

async fn auth_info(State(state): State<Arc<AppState>>,cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let j = json!({"user":get_user(&state,&cookies).await});
    (StatusCode::OK, Json(j)).into_response()
}

async fn list(State(state): State<Arc<AppState>>, Path(id): Path<DbId>, Query(params): Query<HashMap<String, String>>) -> Response {
    let format: String = match params.get("format") {
        Some(s) => s.into(),
        None => "json".into(),
    };
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return (StatusCode::GONE ,Json(json!({"status":format!("Error retrieving list; No list #{id} perhaps?")}))).into_response(),
    };
    let list = list.lock().await;
    let rows = match list.get_rows_for_revision(list.revision_id).await {
        Ok(rows) => rows,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status":e.to_string()}))).into_response(),
    };
    
    match format.as_str() {
        "tsv" => {
            // TODO header
            let rows: Vec<String> = rows.iter().map(|row|row.as_tsv(&list.header)).collect();
            let s = rows.join("\n");
            (StatusCode::OK, s).into_response()
        }
        _ => { // default format: json
            let rows: Vec<serde_json::Value> = rows.iter().map(|row|row.as_json(&list.header)).collect();
            let j = json!({"status":"OK","rows":rows}); // TODO header
            (StatusCode::OK, Json(j)).into_response()
        }
    }
}


async fn run_server(shared_state: Arc<AppState>) -> Result<(), GenericError> {
    tracing_subscriber::fmt::init();

    let cors = CorsLayer::new().allow_origin(Any);

    let app = Router::new()
        .route("/auth/login", get(toolforge_auth))
        .route("/auth/authorized", get(login_authorized))
        .route("/auth/info", get(auth_info))
        .route("/auth/logout", get(logout))

        .route("/list/:id", get(list))

        .merge(SpaRouter::new("/", "html").index_file("index.html"))
        .with_state(shared_state)
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors);
    
    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
    tracing::debug!("listening on {}", addr);
    Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .expect("Server error");

    Ok(())
}

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