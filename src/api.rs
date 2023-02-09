use crate::app_state::AppState;
use crate::oauth::*;
use axum::{
    routing::get,
    Json, 
    Router,
    extract::Path,
    http::StatusCode,
};
use serde_json::json;
use crate::header::DbId;
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
    let j = json!(session).get("data").cloned()?;
    let j = j.get("user").unwrap();
    serde_json::from_str(j.as_str()?).ok()
}

async fn get_user_id(state: &Arc<AppState>,cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<DbId> {
    let user = get_user(&state,&cookies).await?;
    state.get_or_create_wiki_user_id(user.get("username")?.as_str()?).await
}

async fn auth_info(State(state): State<Arc<AppState>>,cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let j = json!({"user":get_user(&state,&cookies).await});
    (StatusCode::OK, Json(j)).into_response()
}

async fn list_info(State(state): State<Arc<AppState>>, Path(id): Path<DbId>, Query(params): Query<HashMap<String, String>>) -> Response {
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return (StatusCode::GONE ,Json(json!({"status":format!("Error retrieving list; No list #{id} perhaps?")}))).into_response(),
    };
    let list = list.lock().await;
    let revision_id: DbId = params.get("revision_id").map(|s|s.parse::<DbId>().unwrap_or(list.revision_id)).unwrap_or(list.revision_id);
    let numer_of_rows = match list.get_rows_in_revision(revision_id).await {
        Ok(numer_of_rows) => numer_of_rows,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status":e.to_string()}))).into_response(),
    };
    let users_in_revision = match list.get_users_in_revision(revision_id).await {
        Ok(users_in_revision) => users_in_revision,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"status":e.to_string()}))).into_response(),
    };
    let j = json!({
        "status":"OK",
        "list":list.to_owned(),
        "users":users_in_revision,
        "total":numer_of_rows
    });
    (StatusCode::OK, Json(j)).into_response()
}

async fn list(State(state): State<Arc<AppState>>, Path(id): Path<DbId>, Query(params): Query<HashMap<String, String>>) -> Response {
    let format: String = params.get("format").unwrap_or(&"json".into()).into();
    let start: u64 = params.get("start").map(|s|s.parse::<u64>().unwrap_or(0)).unwrap_or(0);
    let len: Option<u64> = params.get("len").map(|s|s.parse::<u64>().unwrap_or(u64::MAX));
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return (StatusCode::GONE ,Json(json!({"status":format!("Error retrieving list; No list #{id} perhaps?")}))).into_response(),
    };
    let list = list.lock().await;
    let revision_id: DbId = params.get("revision_id").map(|s|s.parse::<DbId>().unwrap_or(list.revision_id)).unwrap_or(list.revision_id);
    let rows = match list.get_rows_for_revision_paginated(revision_id, start, len).await {
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

async fn my_lists(State(state): State<Arc<AppState>>, Path(rights): Path<String>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user_id = match get_user_id(&state,&cookies).await {
        Some(user_id) => user_id,
        None => return (StatusCode::OK, Json(json!({"status":"Could not get a user ID"}))).into_response()
    };
    let res = state.get_lists_by_user_rights(user_id,&rights).await.unwrap_or(vec![]);
    let j = json!({"status":"OK","lists":res});
    (StatusCode::OK, Json(j)).into_response()
}

pub async fn run_server(shared_state: Arc<AppState>) -> Result<(), GenericError> {
    tracing_subscriber::fmt::init();

    let cors = CorsLayer::new().allow_origin(Any);

    let app = Router::new()
        .route("/auth/login", get(toolforge_auth))
        .route("/auth/authorized", get(login_authorized))
        .route("/auth/info", get(auth_info))
        .route("/auth/logout", get(logout))
        .route("/auth/lists/:rights", get(my_lists))

        .route("/list/:id", get(list))
        .route("/list_info/:id", get(list_info))

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