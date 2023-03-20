use crate::app_state::AppState;
use crate::data_source::{DataSource, DataSourceFormat, DataSourceType};
use crate::file::File;
use crate::gulp_response::ContentType;
use crate::list::List;
use crate::oauth::*;
use crate::header::{DbId, HeaderSchema};
use crate::user::User;
use std::io::prelude::*;
use csv::WriterBuilder;
use serde_json::json;
use tower_http::services::ServeDir;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing;
use tracing_subscriber;
use tower_http::cors::{Any, CorsLayer};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use axum::{
    routing::{get, post},
    Json, 
    Router,
    http::StatusCode,
    Server,
    extract::{Path,State,Query,Multipart,TypedHeader,DefaultBodyLimit},
    response::{IntoResponse, Response},
};
use crate::GulpError;

const MAX_UPLOAD_MB: usize = 50;
const EMPTY_HEADER_SCHEMA_ID: DbId = 3;

async fn auth_info(State(state): State<Arc<AppState>>,cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let j = json!({"status":"OK","user":User::from_cookies(&state, &cookies).await});
    (StatusCode::OK, Json(j)).into_response()
}

async fn list_info(State(state): State<Arc<AppState>>, Path(id): Path<DbId>, Query(params): Query<HashMap<String, String>>,cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return json_error_gone(&format!("Error retrieving list; No list #{id} perhaps?")),
    };
    let list = list.lock().await;
    let revision_id: DbId = params.get("revision_id").map(|s|s.parse::<DbId>().unwrap_or(list.revision_id)).unwrap_or(list.revision_id);
    let numer_of_rows = match list.get_rows_in_revision(revision_id).await {
        Ok(numer_of_rows) => numer_of_rows,
        Err(e) => return json_error(&e.to_string()),
    };
    let users_in_revision = match list.get_users_in_revision(revision_id).await {
        Ok(users_in_revision) => users_in_revision,
        Err(e) => return json_error(&e.to_string()),
    };
    let rights: Vec<String> = match User::from_cookies(&state, &cookies).await {
        Some(user) => user.get_access_for_list(id).await.iter().cloned().collect(),
        None => vec![],
    };
    let j = json!({
        "status":"OK",
        "list":list.to_owned(),
        "users":users_in_revision,
        "total":numer_of_rows,
        "revision_id":revision_id,
        "file_basename":list.get_file_basename(Some(revision_id)),
        "rights":rights,
    });
    (StatusCode::OK, Json(j)).into_response()
}

async fn list_sources(State(state): State<Arc<AppState>>, Path(id): Path<DbId>,) -> Response {
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return json_error_gone(&format!("Error retrieving list; No list #{id} perhaps?")),
    };
    let list = list.lock().await;
    let sources = match list.get_sources().await {
        Ok(sources) => sources,
        Err(e) => return json_error(&format!("Error retrieving list sources: {e}")),
    };
    let user_ids = sources.iter().map(|s|s.user_id).collect();
    let users = match list.get_users_by_id(&user_ids).await {
        Ok(users) => users,
        Err(e) => return json_error(&format!("Error retrieving user details: {e}")),
    };
    let j = json!({"status":"OK","sources":sources,"users":users});
    (StatusCode::OK, Json(j)).into_response()
}

async fn source_header(State(state): State<Arc<AppState>>, Path(source_id): Path<DbId>, Query(_params): Query<HashMap<String, String>>,) -> Response {
    // TODO params with header
    let source = match DataSource::from_db(&state, source_id).await {
        Some(source) => source,
        None => return json_error_gone(&format!("Error retrieving source; No source #{source_id} perhaps?")),
    };
    let cell_set_result = source.guess_headers(Some(50)).await;
    let cell_set = match cell_set_result {
        Ok(cell_set) => cell_set,
        Err(e) => return json_error(&e.to_string()),
    };

    let mut header = crate::header::Header::new();
    header.schema.columns = cell_set.headers;
    let rows: Vec<serde_json::Value> = cell_set.rows.iter().map(|row|row.as_json(&header)).collect();
    let j = json!({"status":"OK","headers":header,"rows":rows});
    (StatusCode::OK, Json(j)).into_response()
}

async fn source_update(State(state): State<Arc<AppState>>, Path(source_id): Path<DbId>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let source = match DataSource::from_db(&state, source_id).await {
        Some(source) => source,
        None => return json_error_gone(&format!("Error retrieving source; No source #{source_id} perhaps?")),
    };
    let list = match AppState::get_list(&state,source.list_id).await {
        Some(list) => list,
        None => return json_error_gone(&format!("Error retrieving list; No list #{} perhaps?",source.list_id)),
    };
    let list = list.lock().await;
    let user = match User::from_cookies(&state, &cookies).await {
        Some(user) => user,
        None => return json_error("Not logged in"),
    };
    if !user.can_update_from_source(list.id).await {
        return json_error(&format!("You are nor allowed to update list {}. Please ask the list admin(s) for permission.",list.id));
    }
    let x = list.update_from_source(&source, user.id).await;
    match x {
        Ok(_) => {}
        Err(e) => return json_error(&format!("Error updating from source: {e}")),
    }
    let j = json!({"status":"OK"});
    (StatusCode::OK, Json(j)).into_response()
}

async fn source_create(State(state): State<Arc<AppState>>, Path(list_id): Path<DbId>, Query(params): Query<HashMap<String, String>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user = match User::from_cookies(&state, &cookies).await {
        Some(user) => user,
        None => return json_error("Not logged in"),
    };
    if !user.can_create_new_data_source(list_id).await {
        return json_error("You are nor allowed to create a new data source for this list. Please ask the list admin(s) for permission.");
    }
    let ds_type = match params.get("type").map(|s|DataSourceType::new(s)) {
        Some(ds_format) => match ds_format {
            Some(ds_format) => ds_format,
            None => return json_error("Invalid type"),
        },
        None => return json_error("Missing type"),
    };
    let ds_format = match params.get("format").map(|s|DataSourceFormat::new(s)) {
        Some(ds_format) => match ds_format {
            Some(ds_format) => ds_format,
            None => return json_error("Invalid format"),
        },
        None => return json_error("Missing format"),
    };

    let mut location = match params.get("location") {
        Some(location) => location.to_owned(),
        None => return json_error("Missing location"),
    };        
    match ds_type { // location contains file ID, NOT the file path. This we need to get from the database; otherwise, the user could ask for any file on disk...
        DataSourceType::FILE => {
            let file_id = match location.parse::<DbId>() {
                Ok(file_id) => file_id,
                Err(_) => return json_error("Locations needs to be file ID"),
            };
            let file = match File::from_id(&state,file_id).await {
                Some(file) => file,
                None => return json_error(&format!("No file ID {file_id} in database")),
            };
            location = file.path.to_string();
        },
        _ => {}
    }

    let mut ds = DataSource {
        id: 0,
        list_id,
        source_type: ds_type,
        source_format: ds_format,
        location,
        user_id: user.id,
    };
    if let None = ds.create(&state).await {
        return json_error("Could not create data source")
    }
    let j = json!({"status":"OK","data":ds});
    (StatusCode::OK, Json(j)).into_response()
}

async fn list_snapshot(State(state): State<Arc<AppState>>, Path(id): Path<DbId>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let list = match AppState::get_list(&state,id).await {
        Some(list) => list,
        None => return json_error_gone(&format!("Error retrieving list; No list #{id} perhaps?")),
    };
    let mut list = list.lock().await;
    let user = match User::from_cookies(&state, &cookies).await {
        Some(user) => user,
        None => return json_error("Not logged in"),
    };
    if !user.can_create_snapshot(list.id).await {
        return json_error("You are nor allowed to create a new snapshot for this list. Please ask the list admin(s) for permission.");
    }
    let old_revision_id = list.revision_id;
    let new_revision_id = match list.snapshot().await {
        Ok(rev_id) => rev_id,
        Err(e) => return json_error_code(StatusCode::INTERNAL_SERVER_ERROR, &format!("Error creating snapshot: {e}")),
    };
    let j = json!({
        "old_revision_id" : old_revision_id,
        "new_revision_id" : new_revision_id,
    });
    (StatusCode::OK, Json(j)).into_response()
}

async fn header_schemas(State(state): State<Arc<AppState>>,) -> Response {
    let hs = match state.get_all_header_schemas().await {
        Ok(hs) => hs,
        Err(e) => return json_error_code(StatusCode::INTERNAL_SERVER_ERROR , &e.to_string()),
    };
    let j = json!({"status":"OK","data":hs});
    (StatusCode::OK, Json(j)).into_response()
}

fn json_error_code(code:StatusCode, s: &str) -> Response {
    (code ,Json(json!({"status":s}))).into_response()
}

fn json_error(s: &str) -> Response {
    json_error_code(StatusCode::OK, s)
}

fn json_error_gone(s: &str) -> Response {
    json_error_code(StatusCode::GONE, s)
}


async fn new_list(State(state): State<Arc<AppState>>, Query(params): Query<HashMap<String, String>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user_id = match User::from_cookies(&state, &cookies).await {
        Some(user) => user.id,
        None => return json_error("Please log in to create a new list"),
    };
    let name = match params.get("name") {
        Some(name) => name.to_owned(),
        None => return json_error("A name is required")
    };
    let header_schema_id = params.get("header_schema_id")
        .map(|s|s.to_owned())
        .unwrap_or_else(||"".to_string())
        .parse::<DbId>()
        .unwrap_or(EMPTY_HEADER_SCHEMA_ID);
    let list = match List::create_new(&state, &name, header_schema_id).await {
        Some(list) => list,
        None => return json_error("New list could not be created"),
    };
    match list.add_access(&state, user_id,"admin").await {
        Ok(_) => {},
        Err(e) => return json_error(&format!("List {} was created, but you could not be added as list admin ({e}).",list.id)),
    }
    let j = json!({"status":"OK","data":list.id});
    (StatusCode::OK, Json(j)).into_response()
}

// list_row
async fn list_row(State(state): State<Arc<AppState>>, Path((list_id,row_num)): Path<(DbId,DbId)>, Query(params): Query<HashMap<String, String>>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user = match User::from_cookies(&state, &cookies).await {
        Some(user) => user,
        None => return json_error("Please log in to set a new header schema for a list"),
    };
    if !user.can_edit_row(list_id).await {
        return json_error("You do not have permission to edit a row in list {list_id}");
    }

    let list = match AppState::get_list(&state,list_id).await {
        Some(list) => list,
        None => return json_error_gone(&format!("Error retrieving list; No list #{list_id} perhaps?")),
    };
    let list = list.lock().await;

    let row_json = match params.get("json") {
        Some(s) => s,
        None => return json_error("No 'json' parameter"),
    };

    let values: serde_json::Value = match serde_json::from_str(row_json) {
        Ok(j) => j,
        Err(e) => return json_error(&e.to_string()),
    };
    let cells = match values["c"].as_array() {
        Some(cells) => cells,
        None => return json_error("Bad JSON"),
    };
    let cells: Vec<Option<crate::cell::Cell>> = cells
        .iter()
        .zip(list.header.schema.columns.iter())
        .map(|(cell,column)|crate::cell::Cell::from_value(cell,column))
        .collect();
    
    let mut row = crate::row::Row::from_cells(cells);
    row.list_id = list_id;
    row.revision_id = list.revision_id;
    row.row_num = row_num;

    let mut conn = match state.get_gulp_conn().await {
        Ok(conn) => conn,
        Err(e) => return json_error(&e.to_string()),
    };
    match row.add_or_replace(&list.header, &mut conn, user.id).await {
        Ok(_) => {},
        Err(e) => return json_error(&e.to_string()),
    }
    
    let j = json!({"status":"OK","row":row.as_json(&list.header)});
    (StatusCode::OK, Json(j)).into_response()
}

async fn list_header_schema(State(state): State<Arc<AppState>>, Path((list_id,header_schema_id)): Path<(DbId,DbId)>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user = match User::from_cookies(&state, &cookies).await {
        Some(user) => user,
        None => return json_error("Please log in to set a new header schema for a list"),
    };
    if !user.can_set_new_header_schema_for_list(list_id).await {
        return json_error("You do not have permission to set a new header schema for list {list_id}");
    }
    let list = match AppState::get_list(&state,list_id).await {
        Some(list) => list,
        None => return json_error_gone(&format!("Error retrieving list; No list #{list_id} perhaps?")),
    };
    let header_schema = match HeaderSchema::from_id_app(&state, header_schema_id).await {
        Ok(header_schema) => header_schema,
        Err(e) => return json_error(&e.to_string()),
    };
    let mut list = list.lock().await;
    match list.set_header_schema(header_schema).await {
        Ok(_) => {},
        Err(e) => return json_error(&e.to_string()),
    }

    let j = json!({"status":"OK"});
    (StatusCode::OK, Json(j)).into_response()
}

async fn new_header_schema(State(state): State<Arc<AppState>>, Query(params): Query<HashMap<String, String>>) -> Response {
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
        Ok(id) => {
            let j = json!({"status":"OK","header_schema_id":id});
            (StatusCode::OK, Json(j)).into_response()        
        }
        Err(e) => json_error(&e.to_string()),
    }
}

fn rows_as_xsv(list: &List, rows: &Vec<crate::row::Row>, delimiter: u8) -> Result<String,GulpError> {
    // TODO header
    let mut wtr = WriterBuilder::new().delimiter(delimiter).from_writer(vec![]);
    for row in rows {
        wtr.write_record(&row.as_vec(&list.header))?;
    }
    let inner = wtr.into_inner().map_err(|e|GulpError::String(e.to_string()))?;
    let ret = String::from_utf8(inner)?;
    Ok(ret)
}

async fn list_rows(State(state): State<Arc<AppState>>, Path(list_id): Path<DbId>, Query(params): Query<HashMap<String, String>>) -> Response {
    let format: String = params.get("format").unwrap_or(&"json".into()).into();
    let start: u64 = params.get("start").map(|s|s.parse::<u64>().unwrap_or(0)).unwrap_or(0);
    let len: Option<u64> = params.get("len").map(|s|s.parse::<u64>().unwrap_or(u64::MAX));
    let list = match AppState::get_list(&state,list_id).await {
        Some(list) => list,
        None => return json_error_gone(&format!("Error retrieving list; No list #{list_id} perhaps?")),
    };
    let list = list.lock().await;
    let revision_id: DbId = params.get("revision_id").map(|s|s.parse::<DbId>().unwrap_or(list.revision_id)).unwrap_or(list.revision_id);
    let rows = match list.get_rows_for_revision_paginated(revision_id, start, len).await {
        Ok(rows) => rows,
        Err(e) => return json_error(&e.to_string()),
    };

    let format = match ContentType::new(&format) {
        Some(format) => format,
        None => return json_error(&format!("Unsupported format: '{format}'")),
    };

    let filename = format!("{}.{}",list.get_file_basename(Some(revision_id)),format.file_ending());
    match format {
        ContentType::CSV => {
            let s = match rows_as_xsv(&list,&rows,b',') {
                Ok(s) => s,
                Err(e) => return json_error(&e.to_string()),
            };
            (format.download_headers(Some(filename)), s).into_response()
        }
        ContentType::TSV => {
            let s = match rows_as_xsv(&list,&rows,b'\t') {
                Ok(s) => s,
                Err(e) => return json_error(&e.to_string()),
            };
            (format.download_headers(Some(filename)), s).into_response()
        }
        ContentType::JSON => { // default format: json
            let rows: Vec<serde_json::Value> = rows.iter().map(|row|row.as_json(&list.header)).collect();
            let j = json!({"status":"OK","rows":rows}); // TODO header
            (format.download_headers(Some(filename)), Json(j)).into_response()
        }
        other => return json_error(&format!("ERROR: Output format '{}' is not supported",other.as_str())),
    }
}

async fn my_lists(State(state): State<Arc<AppState>>, Path(rights): Path<String>, cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user_id = match User::from_cookies(&state, &cookies).await {
        Some(user) => user.id,
        None => return json_error("Please log in to see your lists"),
    };
    let res = state.get_lists_by_user_rights(user_id,&rights).await.unwrap_or(vec![]);
    let j = json!({"status":"OK","lists":res});
    (StatusCode::OK, Json(j)).into_response()
}

async fn upload(State(state): State<Arc<AppState>>, cookies: Option<TypedHeader<headers::Cookie>>, mut multipart: Multipart) -> Response {
    let user_id = match User::from_cookies(&state, &cookies).await {
        Some(user) => user.id,
        None => return json_error("Please log in to upload files"),
    };
    while let Some(field) = multipart.next_field().await.unwrap() {
        let original_filename = field.file_name().unwrap_or("").to_string();
        let data = field.bytes().await.unwrap();
        if !data.is_empty() {
            println!("Length of `{}` is {} bytes", &original_filename, data.len());
            let filename = state.get_new_filename();

            // Write to disk
            let mut file_handle = std::fs::File::create(&filename).unwrap();
            file_handle.write_all(&data).unwrap();
            drop(file_handle);

            let file = match File::create_new(&state, &filename, user_id, &original_filename).await {
                Some(file) => file,
                None => return json_error("Could not create file in database"),
            };
            let j = json!({"status":"OK","file":file});
            return (StatusCode::OK, Json(j)).into_response();
        }
    }
    json_error("No file uploaded")
}


pub async fn run_server(shared_state: Arc<AppState>) -> Result<(), GulpError> {
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
        .route("/list/header_schema/:list_id/:header_schema_id", get(list_header_schema))
        .route("/list/row/:list_id/:row_num", get(list_row))

        .route("/header/schemas", get(header_schemas))
        .route("/header/schema/new", get(new_header_schema))
        
        .route("/source/update/:source_id", get(source_update))
        .route("/source/header/:source_id", get(source_header))
        .route("/source/create/:list_id", get(source_create))

        .route("/upload", post(upload))

        .nest_service("/", ServeDir::new("html"))
        .with_state(shared_state.clone())
        .layer(DefaultBodyLimit::max(1024*1024*MAX_UPLOAD_MB))
        .layer(TraceLayer::new_for_http())
        .layer(CompressionLayer::new())
        .layer(cors);
    
    let port: u16 = shared_state.webserver_port;
    let ip = [0, 0, 0, 0];

    let addr = SocketAddr::from((ip, port));
    tracing::info!("listening on http://{}", addr);
    if let Err(e) = Server::bind(&addr).serve(app.into_make_service()).await {
        return Err(GulpError::String(format!("Server fail: {e}")));
    }
        

    Ok(())
}