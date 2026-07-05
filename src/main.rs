use ferros3::{build_app, build_state, load_config};

#[cfg(target_os = "freebsd")]
use axum::body::{to_bytes, Body};
#[cfg(target_os = "freebsd")]
use axum::http::{HeaderMap, Method, Request, StatusCode};
#[cfg(target_os = "freebsd")]
use axum::response::IntoResponse;
#[cfg(target_os = "freebsd")]
use axum::Router;
#[cfg(target_os = "freebsd")]
use ferros3::blocking_http::{
    body_plan, format_response_head, parse_request_head, read_body, wants_100_continue,
    MAX_BODY_BYTES,
};
#[cfg(target_os = "freebsd")]
use std::{
    io::{self, BufReader, Write},
    net::TcpStream as StdTcpStream,
    sync::atomic::{AtomicUsize, Ordering},
};
#[cfg(target_os = "freebsd")]
use tower::ServiceExt;

/// Cap on concurrent connection threads, so a flood can't spawn unbounded OS threads
/// (a pre-auth resource-exhaustion vector).
#[cfg(target_os = "freebsd")]
const MAX_CONNECTIONS: usize = 512;

/// Upper bound on a buffered response body. The blocking shim buffers the whole response;
/// this prevents an unbounded allocation (the old `to_bytes(body, usize::MAX)`). Fully
/// streaming the body without buffering remains a follow-up.
#[cfg(target_os = "freebsd")]
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024 * 1024; // 2 GiB

#[cfg(not(target_os = "freebsd"))]
#[tokio::main]
async fn main() {
    let config = load_config().await;
    let state = build_state(&config);
    let app = build_app(state);

    let addr = format!("{}:{}", config.endpoint, config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("Failed to bind TCP listener");
    println!("Rust S3 Proxy listening on http://{}", addr);

    axum::serve(listener, app)
        .await
        .expect("server error");
}

#[cfg(target_os = "freebsd")]
#[tokio::main]
async fn main() {
    let config = load_config().await;
    let state = build_state(&config);
    let app = build_app(state);

    let addr = format!("{}:{}", config.endpoint, config.port);
    let listener = std::net::TcpListener::bind(&addr).expect("Failed to bind TCP listener");
    println!("Rust S3 Proxy listening on http://{}", addr);

    let runtime_handle = tokio::runtime::Handle::current();
    static ACTIVE_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

    loop {
        let (stream, peer) = match listener.accept() {
            Ok(conn) => conn,
            Err(e) => {
                eprintln!("Failed to accept socket connection: {:?}", e);
                continue;
            }
        };

        // Shed load past the connection cap instead of spawning threads without bound.
        if ACTIVE_CONNECTIONS.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
            drop(stream);
            continue;
        }
        ACTIVE_CONNECTIONS.fetch_add(1, Ordering::Relaxed);

        let app = app.clone();
        let handle = runtime_handle.clone();
        std::thread::spawn(move || {
            if let Err(e) = serve_blocking_connection(stream, peer, app, handle) {
                eprintln!("Connection error: {:?}", e);
            }
            ACTIVE_CONNECTIONS.fetch_sub(1, Ordering::Relaxed);
        });
    }
}

#[cfg(target_os = "freebsd")]
fn serve_blocking_connection(
    mut stream: StdTcpStream,
    _peer: std::net::SocketAddr,
    app: Router,
    handle: tokio::runtime::Handle,
) -> io::Result<()> {
    stream.set_nodelay(true).ok();
    let (request, method) = read_http_request(&mut stream)?;
    let head_only = method == Method::HEAD;

    handle.block_on(async move {
        let response = app
            .oneshot(request)
            .await
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
        let (parts, body) = response.into_parts();

        // Buffer the body under a hard cap. A mid-stream read error now becomes a clean
        // 500 rather than a bogus empty 200 (the old to_bytes(usize::MAX).unwrap_or_default()).
        match to_bytes(body, MAX_RESPONSE_BYTES).await {
            Ok(body_bytes) => {
                write_http_response(&mut stream, parts.status, &parts.headers, body_bytes, head_only)
            }
            Err(_) => write_http_response(
                &mut stream,
                StatusCode::INTERNAL_SERVER_ERROR,
                &HeaderMap::new(),
                bytes::Bytes::new(),
                head_only,
            ),
        }
    })
}

#[cfg(target_os = "freebsd")]
fn read_http_request(stream: &mut StdTcpStream) -> io::Result<(Request<Body>, Method)> {
    let mut reader = BufReader::new(stream.try_clone()?);

    // Request line + headers (parsed by the host-tested `blocking_http` module).
    let head = parse_request_head(&mut reader)?;

    // Honor `Expect: 100-continue` before reading the body, or standard clients stall
    // waiting for the interim response.
    if wants_100_continue(&head.headers) {
        stream.write_all(b"HTTP/1.1 100 Continue\r\n\r\n")?;
        stream.flush()?;
    }

    // Read the body per its framing (Content-Length or chunked), bounded so a huge
    // declared length can't pre-allocate gigabytes.
    let body = read_body(&mut reader, body_plan(&head.headers), MAX_BODY_BYTES)?;

    let method = head.method.clone();
    let mut builder = Request::builder().method(head.method).uri(head.target);
    if let Some(headers_mut) = builder.headers_mut() {
        *headers_mut = head.headers;
    }
    let request = builder
        .body(Body::from(body))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "failed to build request"))?;

    Ok((request, method))
}

#[cfg(target_os = "freebsd")]
fn write_http_response(
    stream: &mut StdTcpStream,
    status: StatusCode,
    headers: &HeaderMap,
    body_bytes: bytes::Bytes,
    head_only: bool,
) -> io::Result<()> {
    // format_response_head (host-tested) preserves the handler's Content-Length for HEAD
    // instead of overwriting it with the empty body length.
    let head = format_response_head(status, headers, body_bytes.len(), head_only);
    stream.write_all(head.as_bytes())?;
    if !head_only {
        stream.write_all(&body_bytes)?;
    }
    stream.flush()?;
    Ok(())
}
