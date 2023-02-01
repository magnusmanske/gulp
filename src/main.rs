use axum::{
    routing::get,
//    Json, 
    Router,
    response::Html,
//    extract::Path
};
//use serde_json::json;
use std::net::SocketAddr;
use std::env;
use tracing;
use tracing_subscriber;
use tower_http::cors::{Any, CorsLayer};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};


async fn root() -> Html<String> {
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

async fn run_server() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let cors = CorsLayer::new().allow_origin(Any);

    let app = Router::new()
        .route("/", get(root))
/*        .route("/supported_properties", get(supported_properties))
        .route("/item/:prop/:id", get(item))
        .route("/meta_item/:prop/:id", get(meta_item))
        .route("/graph/:prop/:id", get(graph))
        .route("/extend/:item", get(extend)) */
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors);
    
    let addr = SocketAddr::from(([0, 0, 0, 0], 8000));
    tracing::debug!("listening on {}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();

    Ok(())
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let argv: Vec<String> = env::args().collect();
    match argv.get(1).map(|s|s.as_str()) {
        _ => run_server().await?
    }
    Ok(())
}
