use crate::app_state::AppState;
use crate::oauth::*;
use axum::{
    routing::get,
    Json, 
    Router,
    extract::Path,
    http::StatusCode,
};
use crate::header::DbId;
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
use crate::GenericError;

async fn get_user(state: &Arc<AppState>,cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<serde_json::Value> {
    let cookie = cookies.to_owned()?.get(COOKIE_NAME)?.to_string();
    let session = state.store.load_session(cookie).await.ok()??;
    json!(session).get("data").cloned()
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


pub async fn run_server(shared_state: Arc<AppState>) -> Result<(), GenericError> {
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