use crate::app_state::AppState;
use async_session::{MemoryStore, Session, SessionStore};
use axum::{
    async_trait,
    extract::Query,
    extract::State,
    extract::{rejection::TypedHeaderRejectionReason, FromRef, FromRequestParts, TypedHeader},
    http::{header::SET_COOKIE, HeaderMap},
    response::{IntoResponse, Redirect, Response},
    RequestPartsExt,
};
use http::request::Parts;
use oauth2::{reqwest::async_http_client, AuthorizationCode, CsrfToken, TokenResponse};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub static COOKIE_NAME: &str = "SESSION";

// The user data we'll get back from WMF.
#[derive(Debug, Serialize, Deserialize)]
pub struct OAuthUser {
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

pub async fn toolforge_auth(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let (auth_url, _csrf_token) = state
        .oauth_client
        .authorize_url(CsrfToken::new_random)
        //.add_scope(Scope::new("identify".to_string()))
        .url();

    // Redirect to WMF's oauth service
    Redirect::to(auth_url.as_ref())
}

pub async fn logout(
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
pub struct AuthRequest {
    code: String,
    state: String,
}

pub async fn login_authorized(
    Query(query): Query<AuthRequest>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Get an auth token
    let token = state
        .oauth_client
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

    let user_data: OAuthUser = user_data.json::<OAuthUser>().await.unwrap();

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

pub struct AuthRedirect;

impl IntoResponse for AuthRedirect {
    fn into_response(self) -> Response {
        Redirect::temporary("/auth/login").into_response()
    }
}

#[async_trait]
impl<S> FromRequestParts<S> for OAuthUser
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

        let user = session.get::<OAuthUser>("user").ok_or(AuthRedirect)?;

        Ok(user)
    }
}
