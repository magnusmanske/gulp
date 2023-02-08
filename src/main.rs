use clap::{Parser, Subcommand};
use app_state::AppState;
use axum::{
    routing::get,
    Json, 
    Router,
    response::Html,
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
use axum::{
    Server,
    extract::State,
    extract::Query
};


use async_session::{MemoryStore, Session, SessionStore};
use axum::{
    async_trait,
    extract::{
        rejection::TypedHeaderRejectionReason, FromRef, FromRequestParts, TypedHeader,
    },
    http::{header::SET_COOKIE, HeaderMap},
    response::{IntoResponse, Redirect, Response},
    RequestPartsExt,
};
use http::{ request::Parts};
use oauth2::{
    reqwest::async_http_client, AuthorizationCode, CsrfToken, TokenResponse, 
//    basic::BasicClient,  AuthUrl, ClientId, ClientSecret, RedirectUrl, Scope, TokenUrl,
};
use serde::{Deserialize, Serialize};
//use std::{env};
//use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

static COOKIE_NAME: &str = "SESSION";



pub type GenericError = Box<dyn std::error::Error + Send + Sync>;

pub mod app_state;
pub mod database_session_store;
pub mod header;
pub mod cell;
pub mod row;
pub mod list;

async fn get_user(state: &Arc<AppState>,cookies: &Option<TypedHeader<headers::Cookie>>) -> Option<String> {
    let cookies = match cookies {
        Some(cookies) => cookies,
        None => return None,
    };
    let cookie = cookies.get(COOKIE_NAME).unwrap();
    match state.store.load_session(cookie.to_string()).await.unwrap() {
        Some(session) => {
            let user_opt: Option<User> = session.get("user");
            Some(user_opt?.username)
        }
        None => None
    }
}

fn user_box(user: &Option<String>) -> String {
    let ret = match user {
        Some(username) => format!("Welcome, {username}!"),
        None => "<a href='/auth/login'>Log in</a>".to_string()
    };
    format!("<div style='float:right;'>{ret}</div>")
}

async fn root(State(state): State<Arc<AppState>>,cookies: Option<TypedHeader<headers::Cookie>>,) -> Response {
    let user = get_user(&state,&cookies).await;

    let html = r##"__USERBOX__
    <h1>GULP</h1>
    <p>General Unified List Processor</p>
    "##;
    let html = html.replace("__USERBOX__",&user_box(&user));
    Html(html).into_response()
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


async fn run_server(shared_state: Arc<AppState>) -> Result<(), GenericError> {
    tracing_subscriber::fmt::init();

    let cors = CorsLayer::new().allow_origin(Any);

    let app = Router::new()
        .route("/", get(root))
        //.route("/auth/login", get(auth_login))
        .route("/list/:id", get(list))

        .route("/auth/login", get(toolforge_auth))
        .route("/auth/authorized", get(login_authorized))
        //.route("/protected", get(protected))
        .route("/logout", get(logout))

        //.route("/auth/authorized", login_authorized())
/*        .route("/meta_item/:prop/:id", get(meta_item))
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




// The user data we'll get back from toolforge.
#[derive(Debug, Serialize, Deserialize)]
struct User {
    pub username: String,
    realname: String,
    email: String,
    editcount: u64,
    confirmed_email: bool,
    blocked: bool,
    groups: Vec<String>,
    rights: Vec<String>,
    grants: Vec<String>,
}
/*
// Session is optional
async fn index(user: Option<User>) -> impl IntoResponse {
    match user {
        Some(u) => format!(
            "Hey {}! You're logged in!\nYou may now access `/protected`.\nLog out with `/logout`.",
            u.username
        ),
        None => "You're not logged in.\nVisit `/auth/login` to do so.".to_string(),
    }
}
 */
async fn toolforge_auth(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let (auth_url, _csrf_token) = state.oauth_client
        .authorize_url(CsrfToken::new_random)
        //.add_scope(Scope::new("identify".to_string()))
        .url();

    // Redirect to toolforge's oauth service
    Redirect::to(auth_url.as_ref())
}
/*
// Valid user session required. If there is none, redirect to the auth page
async fn protected(State(state): State<Arc<AppState>>, user: User) -> impl IntoResponse {
    format!(
        "Welcome to the protected area :)\nHere's your info:\n{:?}",
        user
    )
}
 */
async fn logout(
    State(state): State<Arc<AppState>>,
    TypedHeader(cookies): TypedHeader<headers::Cookie>,
) -> impl IntoResponse {
    let cookie = cookies.get(COOKIE_NAME).unwrap();
    let session = match state.store.load_session(cookie.to_string()).await.unwrap() {
        Some(s) => s,
        // No session active, just redirect
        None => return Redirect::to("/"),
    };

    state.store.destroy_session(session).await.unwrap();

    Redirect::to("/")
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct AuthRequest {
    code: String,
    state: String,
}

async fn login_authorized(
    Query(query): Query<AuthRequest>,
    State(state): State<Arc<AppState>>
) -> impl IntoResponse {
    // Get an auth token
    let token = state.oauth_client
        .exchange_code(AuthorizationCode::new(query.code.clone()))
        .request_async(async_http_client)
        .await
        .unwrap();

    // Fetch user data from toolforge
    let client = reqwest::Client::new();
    let user_data = client
        .get("https://meta.wikimedia.org/w/rest.php/oauth2/resource/profile")
        .bearer_auth(token.access_token().secret())
        .send()
        .await
        .unwrap();

    let user_data: User = user_data
        .json::<User>()
        .await
        .unwrap();

    // Create a new session filled with user data
    let mut session = Session::new();
    session.insert("user", &user_data).unwrap();

    // Store session and get corresponding cookie
    let cookie = state.store.store_session(session).await.unwrap().unwrap();

    // Build the cookie
    let cookie = format!("{}={}; SameSite=Lax; Path=/", COOKIE_NAME, cookie);

    // Set cookie
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());

    (headers, Redirect::to("/"))
}

struct AuthRedirect;

impl IntoResponse for AuthRedirect {
    fn into_response(self) -> Response {
        Redirect::temporary("/auth/login").into_response()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for User
where
    MemoryStore: FromRef<S>,
    S: Send + Sync,
{
    // If anything goes wrong or no session is found, redirect to the auth page
    type Rejection = AuthRedirect;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let store = MemoryStore::from_ref(state);

        let cookies = parts
            .extract::<TypedHeader<headers::Cookie>>()
            .await
            .map_err(|e| match *e.name() {
                http::header::COOKIE => match e.reason() {
                    TypedHeaderRejectionReason::Missing => AuthRedirect,
                    _ => panic!("unexpected error getting Cookie header(s): {}", e),
                },
                _ => panic!("unexpected error getting cookies: {}", e),
            })?;
        let session_cookie = cookies.get(COOKIE_NAME).ok_or(AuthRedirect)?;

        let session = store
            .load_session(session_cookie.to_string())
            .await
            .unwrap()
            .ok_or(AuthRedirect)?;

        let user = session.get::<User>("user").ok_or(AuthRedirect)?;

        Ok(user)
    }
}


/*
ssh magnus@tools-login.wmflabs.org -L 3308:tools-db:3306 -N &
*/