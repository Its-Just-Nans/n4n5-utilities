//! Sharing web server

use axum::{
    Router,
    extract::{ConnectInfo, DefaultBodyLimit, Multipart, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
};
use mime_guess::from_path;
use std::{fmt::Write, net::SocketAddr, path::PathBuf, time::SystemTime};
use std::{fs, net::UdpSocket, sync::Arc};
use tokio::fs as tokio_fs;

/// Upload dir
const UPLOAD_DIR: &str = "uploads";
/// Port for server
const PORT: u16 = 8000;

/// Server state
#[derive(Clone)]
pub struct AppState {
    /// Upload dir
    pub upload_dir: PathBuf,
}

/// main share function
/// # Errors
/// Return error if the server fails
pub async fn cli_main() -> std::io::Result<()> {
    let upload_dir = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from(UPLOAD_DIR), PathBuf::from);
    let state = Arc::new(AppState { upload_dir });
    main_from_app_state(state).await?;
    Ok(())
}

/// Main
/// # Errors
/// Return error if the server fails
pub async fn main_from_app_state(state: Arc<AppState>) -> std::io::Result<()> {
    let upload_dir = state.upload_dir.clone();
    let app = Router::new()
        .route("/", get(index))
        .route("/upload", post(upload))
        .route("/files/{name}", get(download))
        .route("/view/{name}", get(view))
        .layer(DefaultBodyLimit::max(128 * 1024 * 1024)) // 128Mib
        .with_state(state);

    let addr = format!("0.0.0.0:{PORT}");

    println!("==================================================");
    println!(" File Upload Server - {}", upload_dir.display());
    println!("==================================================");
    println!("Localhost : http://localhost:{PORT}");
    println!("Loopback  : http://127.0.0.1:{PORT}");
    println!("LAN       : http://{}:{PORT}", local_ip()?);
    println!("==================================================");

    let listener = tokio::net::TcpListener::bind(addr).await?;

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// Get the local ip
/// # Errors
/// Return errors if cannot connect
fn local_ip() -> std::io::Result<String> {
    let sock = UdpSocket::bind("0.0.0.0:0")?;
    sock.connect("8.8.8.8:80")?;
    let ip = sock.local_addr()?.ip().to_string();
    Ok(ip)
}

/// Show the index
async fn index(State(state): State<Arc<AppState>>) -> Html<String> {
    let mut files_html = String::new();

    if let Ok(entries) = fs::read_dir(&state.upload_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let _ = write!(files_html, "<li><a href=\"/files/{name}\">{name}</a></li>");
        }
    }

    Html(format!(
        r#"
<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<title>Upload Server</title>
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<style>
body {{ font-family: Arial; background:#f3f3f3; margin:40px; }}
.container {{ max-width:700px; margin:auto; background:white; padding:20px; border-radius:10px; }}
.drop {{ border:3px dashed #888; padding:40px; text-align:center; margin:20px 0; }}
</style>
</head>
<body>
<div class="container">
<h2>📁 File Upload Server - uses HTTP (without S)</h2>

<form action="/upload" method="post" enctype="multipart/form-data">
<div class="drop">
<input type="file" name="file" multiple>
</div>
<button type="submit">Upload</button>
</form>
<details>
<summary>Text upload </summary>
<form action="/upload" method="post" enctype="multipart/form-data">
<input type="text" name="file">
<button type="submit">Upload</button>
</form>
</details>

<h3>Files</h3>
<ul style="overflow-wrap: break-word;">
{files_html}
</ul>
</div>
</body>
</html>
"#
    ))
}

/// Upload function
async fn upload(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    loop {
        let new_field = match multipart.next_field().await {
            Ok(f) => f,
            Err(err) => {
                let msg = "Error reading multipart field";
                eprintln!("{} - {msg}: {err}", addr.ip());
                return (StatusCode::BAD_REQUEST, msg).into_response();
            }
        };
        let Some(field) = new_field else {
            break;
        };
        let name = if let Some(n) = field.file_name() {
            sanitize(n)
        } else {
            let duration = match SystemTime::now().duration_since(SystemTime::UNIX_EPOCH) {
                Ok(n) => format!("{}", n.as_secs()),
                Err(_e) => "no_timestamp".to_string(),
            };
            format!("text_{duration}.txt")
        };

        if name.is_empty() {
            let msg = "Empty file name";
            eprintln!("{} - {msg}", addr.ip());
            return (StatusCode::BAD_REQUEST, msg).into_response();
        }
        let data = match field.bytes().await {
            Ok(data) => data,
            Err(e) => {
                eprintln!("{} - Failed to read bytes of {name}: {e}", addr.ip());
                return (
                    StatusCode::BAD_REQUEST,
                    "Failed to read bytes - file too big?",
                )
                    .into_response();
            }
        };
        let path = state.upload_dir.join(name);

        println!(
            "{}: receiving {}: {} bytes",
            addr.ip(),
            path.display(),
            data.len()
        );
        if let Err(err) = fs::create_dir_all(&state.upload_dir) {
            eprintln!(
                "Failed to create the upload folder {}: {err}",
                state.upload_dir.display()
            );
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
        if tokio_fs::try_exists(&path).await.is_ok_and(|res| res) {
            let msg = format!("Path already exists: '{}'", path.display());
            eprintln!("{} - {msg}", addr.ip());
            return (StatusCode::BAD_REQUEST, msg).into_response();
        }
        match tokio_fs::write(&path, data).await {
            Ok(b) => b,
            Err(err) => {
                let msg = format!("Failed to write '{}'", path.display());
                eprintln!("{} - {msg}: {err}", addr.ip());
                return (StatusCode::BAD_REQUEST, msg).into_response();
            }
        }
    }
    Redirect::to("/").into_response()
}

/// Send back the file
async fn action(name: String, path: String, should_download: bool) -> impl IntoResponse {
    match tokio_fs::read(&path).await {
        Ok(data) => {
            let mime = from_path(&path).first_or_octet_stream();

            let response = Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime.as_ref());
            let response = if should_download {
                response.header(
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", sanitize(&name)),
                )
            } else {
                response
            };
            match response.body(axum::body::Body::from(data)) {
                Ok(resp) => resp,
                Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            }
        }

        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// Download the file
async fn download(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl IntoResponse {
    let path = format!("{UPLOAD_DIR}/{}", sanitize(&name));
    eprintln!("{} is downloading {path}", addr.ip());
    action(name, path, true).await
}

/// View the file
async fn view(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl IntoResponse {
    let path = format!("{UPLOAD_DIR}/{}", sanitize(&name));
    eprintln!("{} is downloading {path}", addr.ip());
    action(name, path, false).await
}

/// Quick and dirty sanitize
fn sanitize(name: &str) -> String {
    name.replace('/', "_").replace("..", "_")
}
