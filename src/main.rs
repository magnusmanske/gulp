//#[macro_use]
extern crate serde_json;

pub mod list ;
pub mod app_state;
pub mod gulp_response;

use tokio::fs::File as TokioFile;
use tokio_util::codec::{BytesCodec, FramedRead};
//use qstring::QString;
//use crate::form_parameters::FormParameters;
use app_state::AppState;
use gulp_response::{ContentType,GulpResponse};
//use platform::{MyResponse, Platform, ContentType};
use serde_json::Value;
use std::env;
use std::fs::File;
use std::sync::Arc;
use std::{net::SocketAddr};
use hyper::{header, Body, Request, Response, Server, Error, StatusCode, Method};
use hyper::service::{make_service_fn, service_fn};

static NOTFOUND: &[u8] = b"Not Found";

async fn process_form(parameters:&str, state: Arc<AppState>) -> GulpResponse {
    GulpResponse {
        s:"this is a GulpResponse".to_string(),
        content_type:ContentType::Plain
    }
}

/// HTTP status code 404
fn not_found() -> Result<Response<Body>,Error> {
    Ok(Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(NOTFOUND.into())
        .unwrap())
}

async fn simple_file_send(filename: &str,content_type: &str) -> Result<Response<Body>,Error> {
    // Serve a file by asynchronously reading it by chunks using tokio-util crate.
    let filename = format!("html{}",filename);
    if let Ok(file) = TokioFile::open(filename).await {
        let stream = FramedRead::new(file, BytesCodec::new());
        let body = Body::wrap_stream(stream);
        let response = Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap();
        return Ok(response);
    }

    not_found()
}

async fn serve_file_path(filename:&str) -> Result<Response<Body>,Error> {
    match filename {
        "/" => simple_file_send("/index.html","text/html; charset=utf-8").await,
        "/index.html" => simple_file_send(filename,"text/html; charset=utf-8").await,
        "/main.js" => simple_file_send(filename,"application/javascript; charset=utf-8").await,
        "/favicon.ico" => simple_file_send(filename,"image/x-icon; charset=utf-8").await,
        "/robots.txt" => simple_file_send(filename,"text/plain; charset=utf-8").await,
_ => not_found()
    }
}

async fn process_from_query(query:&str,app_state:Arc<AppState>) -> Result<Response<Body>,Error> {
    let ret = process_form(query,app_state).await;
    let response = Response::builder()
        .header(header::CONTENT_TYPE, ret.content_type.as_str())
        .body(Body::from(ret.s))
        .unwrap();
    Ok(response)
}

async fn process_request(mut req: Request<Body>,app_state:Arc<AppState>) -> Result<Response<Body>,Error> {
    // URL GET query
    if let Some(query) = req.uri().query() {
        if !query.is_empty() {
            return process_from_query(query,app_state).await;
        }
    } ;

    // POST
    if req.method() == Method::POST {
        let query = hyper::body::to_bytes(req.body_mut()).await.unwrap();
        if !query.is_empty() {
            let query = String::from_utf8_lossy(&query);
            return process_from_query(&query,app_state).await;
        }
    }

    // Fallback: Static file
    serve_file_path(req.uri().path()).await
}


#[tokio::main]
async fn main() -> Result<(),Error> {
    let basedir = env::current_dir()
        .expect("Can't get CWD")
        .to_str()
        .expect("Can't convert CWD to_str")
        .to_string();
    let path = basedir.to_owned() + "/config.json";
    let file = File::open(&path).unwrap_or_else(|_| panic!("Can not open config file at {}", &path));
    let petscan_config: Value =
        serde_json::from_reader(file).expect("Can not parse JSON from config file");

    let ip_address = petscan_config["http_server"].as_str().unwrap_or("0.0.0.0").to_string();
    let port = petscan_config["http_port"].as_u64().unwrap_or(80) as u16;
    let app_state = Arc::new(AppState::new_from_config(&petscan_config).await) ;

    let ip_address : Vec<u8> = ip_address.split('.').map(|s|s.parse::<u8>().unwrap()).collect();
    let ip_address = std::net::Ipv4Addr::new(ip_address[0],ip_address[1],ip_address[2],ip_address[3],);
    let addr = SocketAddr::from((ip_address, port));

    let make_service = make_service_fn(move |_| {
        let app_state = app_state.clone();

        async {
            Ok::<_, Error>(service_fn(move |req|  {
                process_request(req,app_state.to_owned())
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_service);

    println!("Listening on http://{}", addr);

    server.await?;

    Ok(())
}
