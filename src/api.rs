use crate::app_state::AppState;
use crate::data_source::DataSource;
use crate::list::List;
use crate::oauth::*;
use crate::header::DbId;
use mysql_async::prelude::*;
use csv::WriterBuilder;use serde_json::json;
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
    routing::{get, post},
    Json, 
    Router,
    http::StatusCode,
    Server,
    extract::{Path,State,Query,Multipart,TypedHeader,DefaultBodyLimit},
    response::{IntoResponse, Response},
};
use crate::GenericError;

async fn get_user(state: &Arc<AppState>,cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<serde_json::Value> {
    let cookie = cookies.to_owned()?.get(COOKIE_NAME)?.to_string();
    let session = state.store.load_session(cookie).await.ok()??;
    let j = json!(session).get("data").cloned()?;
    let mut j = j.get("user")?.to_owned();
    j["id"] = json!(state.get_or_create_wiki_user_id(j.get("username")?.as_str()?).await?);
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
        "total":numer_of_rows,
        "revision_id":revision_id,
    });
    (StatusCode::OK, Json(j)).into_response()
}

async fn list_sources(State(state): State<Arc<AppState>>, Path(id): Path<DbId>,) -> Response {
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return (StatusCode::GONE ,Json(json!({"status":format!("Error retrieving list; No list #{id} perhaps?")}))).into_response(),
    };
    let list = list.lock().await;
    let sources = match list.get_sources().await {
        Ok(sources) => sources,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR ,Json(json!({"status":format!("Error retrieving list sources: {}",e.to_string())}))).into_response(),
    };
    let user_ids = sources.iter().map(|s|s.user_id).collect();
    let users = match list.get_users_by_id(&user_ids).await {
        Ok(users) => users,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR ,Json(json!({"status":format!("Error retrieving user details: {}",e.to_string())}))).into_response(),
    };
    let j = json!({"status":"OK","sources":sources,"users":users});
    (StatusCode::OK, Json(j)).into_response()
}

async fn source_update(State(state): State<Arc<AppState>>, Path(source_id): Path<DbId>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user_id = match get_user_id(&state,&cookies).await {
        Some(user_id) => user_id,
        None => return (StatusCode::OK, Json(json!({"status":"Could not get a user ID"}))).into_response()
    };
    let source = match DataSource::from_db(&state, source_id).await {
        Some(source) => source,
        None => return (StatusCode::GONE ,Json(json!({"status":format!("Error retrieving source; No source #{source_id} perhaps?")}))).into_response(),
    };
    let list = match AppState::get_list(&state,source.list_id).await {
        Some(list) => list,
        None => return (StatusCode::GONE ,Json(json!({"status":format!("Error retrieving list; No list #{} perhaps?",source.list_id)}))).into_response(),
    };
    let list = list.lock().await;
    match list.update_from_source(&source, user_id).await {
        Ok(_) => {}
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR ,Json(json!({"status":format!("Error updating from source: {}",e.to_string())}))).into_response(),
    }
    let j = json!({"status":"OK"});
    (StatusCode::OK, Json(j)).into_response()
}

async fn list_snapshot(State(state): State<Arc<AppState>>, Path(id): Path<DbId>) -> Response {
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return (StatusCode::GONE ,Json(json!({"status":format!("Error retrieving list; No list #{id} perhaps?")}))).into_response(),
    };
    let mut list = list.lock().await;
    let old_revision_id = list.revision_id;
    let new_revision_id = match list.snapshot().await {
        Ok(rev_id) => rev_id,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR ,Json(json!({"status":format!("Error creating snapshot: {}",e.to_string())}))).into_response(),
    };
    let j = json!({
        "old_revision_id" : old_revision_id,
        "new_revision_id" : new_revision_id,
    });
    (StatusCode::OK, Json(j)).into_response()
}

async fn header_schemas(State(state): State<Arc<AppState>>,) -> Response {
    let sql = r#"SELECT header_schema.id,name,json FROM header_schema WHERE id>:dummy"#;
    let dummy = 0;
    let hs: Vec<crate::header::HeaderSchema> = state.get_gulp_conn().await.unwrap()
        .exec_iter(sql,params! {dummy}).await.unwrap()
        .map_and_drop(|row| crate::header::HeaderSchema::from_row(&row)).await.unwrap()
        .iter().filter_map(|s|s.to_owned()).collect();
    let j = json!({"status":"OK","data":hs});
    (StatusCode::OK, Json(j)).into_response()
}

fn json_error(s: &str) -> Response {
    (StatusCode::OK ,Json(json!({"status":s}))).into_response()
}

async fn new_list(State(state): State<Arc<AppState>>, Query(params): Query<HashMap<String, String>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user_id = match get_user_id(&state,&cookies).await {
        Some(user_id) => user_id,
        None => return json_error("You need to be logged in")
    };
    let name = match params.get("name") {
        Some(name) => name.to_owned(),
        None => return json_error("A name is required")
    };
    let header_schema_id = match params.get("header_schema_id") {
        Some(s) => {
            match s.parse::<DbId>() {
                Ok(id) => id,
                Err(e) => return json_error(&e.to_string()),
            }
        },
        None => return json_error("A header_schema_id is required"),
    };
    let list = match List::create_new(&state, &name, header_schema_id).await {
        Some(list) => list,
        None => return json_error("New list could not be created"),
    };
    let _ = list.add_access(&state, user_id,"admin").await;
    let j = json!({"status":"OK","data":list.id});
    (StatusCode::OK, Json(j)).into_response()
}

async fn new_header_schema(State(state): State<Arc<AppState>>, Query(params): Query<HashMap<String, String>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let _user_id = match get_user_id(&state,&cookies).await {
        Some(user_id) => user_id,
        None => return json_error("You need to be logged in")
    };
    let name = match params.get("name") {
        Some(name) => name.to_owned(),
        None => return json_error("A name is required")
    };
    let json_string = match params.get("json") {
        Some(json_string) => json_string.to_owned(),
        None => return json_error("JSON is required")
    };
    let json: serde_json::Value = match serde_json::from_str(&json_string) {
        Ok(json) => json,
        Err(e) => return json_error(&e.to_string())
    };
    let mut hs = match crate::header::HeaderSchema::from_name_json(&name, &json.to_string()) {
        Some(hs) => hs,
        None => return json_error(&format!("Invalid JSON: {json:?}"))
    };
    match hs.create_in_db(&state).await {
        Ok(0) => json_error("INSERT was run but no new ID was returned"),
        Ok(_id) => json_error("OK"),
        Err(e) => json_error(&e.to_string()),
    }
}

async fn list_rows(State(state): State<Arc<AppState>>, Path(id): Path<DbId>, Query(params): Query<HashMap<String, String>>) -> Response {
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
        "csv" => {
            // TODO header
            let mut wtr = WriterBuilder::new().from_writer(vec![]);
            for row in &rows {
                wtr.write_record(&row.as_vec(&list.header)).unwrap();
            }
            let inner = wtr.into_inner().unwrap();
            let s = String::from_utf8(inner).unwrap();
            (StatusCode::OK, s).into_response()
        }
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

async fn upload(mut multipart: Multipart) {
    while let Some(field) = multipart.next_field().await.unwrap() {
        let name = field.name().unwrap().to_string();
        let data = field.bytes().await.unwrap();

        println!("Length of `{}` is {} bytes", name, data.len());
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
        .route("/auth/lists/:rights", get(my_lists))

        .route("/list/rows/:id", get(list_rows))
        .route("/list/info/:id", get(list_info))
        .route("/list/snapshot/:id", get(list_snapshot))
        .route("/list/sources/:id", get(list_sources))
        .route("/list/new", get(new_list))

        .route("/header/schemas", get(header_schemas))
        .route("/header/schema/new", get(new_header_schema))
        

        .route("/source/update/:source_id", get(source_update))

        .route("/upload", post(upload))

        .merge(SpaRouter::new("/", "html").index_file("index.html"))
        .with_state(shared_state)
        .layer(DefaultBodyLimit::max(1024*1024*8)) // 8MB
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