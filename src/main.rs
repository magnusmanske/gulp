use app_state::AppState;
use axum::{
    routing::get,
//    Json, 
    Router,
    response::Html,
//    extract::Path
};
use std::env;
//use serde_json::json;
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

//use crate::list::FileType;

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
/*        .route("/supported_properties", get(supported_properties))
        .route("/item/:prop/:id", get(item))
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
#[tokio::main]
async fn main() -> Result<(), GenericError> {
    let app = Arc::new(AppState::from_config_file("config.json").expect("app creation failed"));

    let list = app.get_list(4).await.expect("List does not exists");
    let mut list = list.lock().await;
    println!("{list:?}");
    // let _ = list.import_from_url(&mut conn,"https://wikidata-todo.toolforge.org/file_candidates_hessen.txt",FileType::JSONL).await;
    let mut conn = app.get_gulp_conn().await?;
    let revision_id = list.snapshot(&mut conn).await?;
    println!("{revision_id:?}");

    if false {
        let argv: Vec<String> = env::args().collect();
        match argv.get(1).map(|s|s.as_str()) {
            _ => run_server(app).await?
        }
    }
    
    Ok(())
}


/*
ssh magnus@tools-login.wmflabs.org -L 3308:tools-db:3306 -N &
*/