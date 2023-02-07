use axum::extract::Query;
use clap::{Parser, Subcommand};
use app_state::AppState;
use axum::{
    routing::get,
    Json, 
    Router,
    response::Html,
    extract::Path,
    http::StatusCode,
    response::{IntoResponse, Response},
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
use axum::{
    Server,
    extract::State
};

pub type GenericError = Box<dyn std::error::Error + Send + Sync>;

pub mod app_state;
pub mod header;
pub mod cell;
pub mod row;
pub mod list;


async fn root(State(_state): State<Arc<AppState>>,) -> Html<String> {
    let html = r##"<h1>GULP</h1>
    "##;
    Html(html.into())
}

async fn list(State(state): State<Arc<AppState>>, Path(id): Path<DbId>, Query(params): Query<HashMap<String, String>>) -> Response {
    let format: String = match params.get("format") { // TODO use format
        Some(s) => s.into(),
        None => "json".into(),
    };
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return (StatusCode::GONE ,Json(json!({"status":format!("Error retrieving list; No list #{id} perhaps?")}))).into_response(),
    };
    let list = list.lock().await;
    //let revision_id = list.snapshot().await?;
    //list.import_from_url("https://wikidata-todo.toolforge.org/file_candidates_hessen.txt",list::FileType::JSONL).await?;
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
        _ => {
            let rows: Vec<serde_json::Value> = rows.iter().map(|row|row.as_json(&list.header)).collect();
            let j = json!({"status":"OK","rows":rows}); // TODO header
            (StatusCode::OK, Json(j)).into_response()        
        }
    }
}

/*
async fn item(Path((property,id)): Path<(String,String)>) -> Json<serde_json::Value> {
    let parser: Box<dyn ExternalImporter> = match Combinator::get_parser_for_property(&property, &id) {
        Ok(parser) => parser,
        Err(e) => return Json(json!({"status":e.to_string()}))
    };
    let mi = match parser.run() {
        Ok(mi) => mi,
        Err(e) => return Json(json!({"status":e.to_string()}))
    };
    let mut j = json!(mi)["item"].to_owned();
    j["status"] = json!("OK");
    Json(j)
}

async fn meta_item(Path((property,id)): Path<(String,String)>) -> Json<serde_json::Value> {
    let parser: Box<dyn ExternalImporter> = match Combinator::get_parser_for_property(&property, &id) {
        Ok(parser) => parser,
        Err(e) => return Json(json!({"status":e.to_string()}))
    };
    let mi = match parser.run() {
        Ok(mi) => mi,
        Err(e) => return Json(json!({"status":e.to_string()}))
    };
    let mut j = json!(mi);
    j["status"] = json!("OK");
    Json(j)
}

async fn graph(Path((property,id)): Path<(String,String)>) -> String {
    let mut parser: Box<dyn ExternalImporter> = match Combinator::get_parser_for_property(&property, &id) {
        Ok(parser) => parser,
        Err(e) => return e.to_string()
    };
    parser.get_graph_text()
}

async fn extend(Path(item): Path<String>) -> Json<serde_json::Value> {
    let mut base_item = match meta_item::MetaItem::from_entity(&item).await {
        Ok(base_item) => base_item,
        Err(e) => return Json(json!({"status":e.to_string()}))
    };
    let ext_ids: Vec<ExternalId> = base_item
        .get_external_ids()
        .iter()
        .filter(|ext_id|Combinator::get_parser_for_ext_id(ext_id).ok().is_some())
        .cloned()
        .collect();
    let mut combinator = Combinator::new();
    if let Err(e) = combinator.import(ext_ids) {
        return Json(json!({"status":e.to_string()}))
    }
    let other = match combinator.combine() {
        Some(other) => other,
        None => return Json(json!({"status":"No items to combine"}))
    };
    let diff = base_item.merge(&other);
    Json(json!(diff))
}

async fn supported_properties() -> Json<serde_json::Value> {
    let ret: Vec<String> = Combinator::get_supported_properties()
        .iter()
        .map(|prop|format!("P{prop}"))
        .collect();
        Json(json!(ret))
}
 */

async fn run_server(shared_state: Arc<AppState>) -> Result<(), GenericError> {
    tracing_subscriber::fmt::init();

    let cors = CorsLayer::new().allow_origin(Any);

    let app = Router::new()
        .route("/", get(root))
        .route("/list/:id", get(list))
/*        .route("/item/:prop/:id", get(item))
        .route("/meta_item/:prop/:id", get(meta_item))
        .route("/graph/:prop/:id", get(graph))
        .route("/extend/:item", get(extend)) */
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
    // println!("{:?}",&cli.command);

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