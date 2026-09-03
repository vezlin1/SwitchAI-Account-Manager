use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use url::Url;

use crate::app_state::{SharedState, lock_flows};
use crate::errors::{AppError, AppResult};
use crate::models::now_ts;
use crate::providers::chatgpt::oauth::{OauthFlowStatus, complete_oauth_code, prune_oauth_flows};

pub const CALLBACK_ADDR: &str = "127.0.0.1:1455";
pub const OAUTH_REDIRECT_URI: &str = "http://localhost:1455/auth/callback";

const CALLBACK_IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CALLBACK_REQUEST_BYTES: usize = 16 * 1024;
const MAX_CALLBACK_URL_BYTES: usize = 2 * 1024;
const MAX_CALLBACK_CONNECTIONS: usize = 8;
pub const MAX_OAUTH_SUCCESS_BODY_BYTES: usize = 256 * 1024;
pub const MAX_OAUTH_ERROR_BODY_BYTES: usize = 64 * 1024;

static ACTIVE_CALLBACK_CONNECTIONS: AtomicUsize = AtomicUsize::new(0);

// Serializes the synchronous bind decision. Concurrent starters either perform
// the bind themselves or observe the successfully running listener; none can
// return success while another thread is still attempting to bind.
static CALLBACK_SERVER_START_LOCK: Mutex<()> = Mutex::new(());

pub fn ensure_callback_server(shared: &Arc<SharedState>) -> AppResult<()> {
    let _start_guard = CALLBACK_SERVER_START_LOCK
        .lock()
        .map_err(|_| AppError::msg("OAuth callback server start lock is poisoned"))?;
    if shared.callback_server_started.load(Ordering::SeqCst) {
        return Ok(());
    }

    let listener = bind_callback_listener().inspect_err(|error| {
        log::warn!("{}", error.user_message());
    })?;
    shared.callback_server_started.store(true, Ordering::SeqCst);

    let shared = Arc::clone(shared);
    thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    let connection_state = Arc::clone(&shared);
                    thread::spawn(move || {
                        if let Err(err) = handle_callback_stream(stream, &connection_state) {
                            log::warn!("OAuth callback request failed: {}", err.user_message());
                        }
                    });
                }
                Err(err) => {
                    log::warn!("OAuth callback accept failed: {err}");
                    if shared.callback_server_started.load(Ordering::SeqCst) {
                        shared
                            .callback_server_started
                            .store(false, Ordering::SeqCst);
                    }
                    return;
                }
            }
        }

        shared
            .callback_server_started
            .store(false, Ordering::SeqCst);
    });
    Ok(())
}

pub(crate) fn bind_callback_listener() -> AppResult<TcpListener> {
    TcpListener::bind(CALLBACK_ADDR).map_err(|source| AppError::Io {
        context: "Failed to bind OAuth callback server on 127.0.0.1:1455",
        source,
    })
}

pub fn write_http_response(stream: &mut TcpStream, status: &str, body: &str) -> AppResult<()> {
    write_http_response_with_limit(stream, status, body, MAX_OAUTH_SUCCESS_BODY_BYTES)
}

pub fn write_error_http_response(
    stream: &mut TcpStream,
    status: &str,
    title: &str,
    message: &str,
) -> AppResult<()> {
    let body = html_message(title, message);
    if body.len() > MAX_OAUTH_ERROR_BODY_BYTES {
        return Err(AppError::msg(
            "OAuth callback response body exceeds the configured limit",
        ));
    }
    write_http_response_with_limit(stream, status, &body, MAX_OAUTH_ERROR_BODY_BYTES)
}

pub fn write_http_response_with_limit(
    stream: &mut TcpStream,
    status: &str,
    body: &str,
    max_bytes: usize,
) -> AppResult<()> {
    if body.len() > max_bytes {
        return Err(AppError::msg(
            "OAuth callback response body exceeds the configured limit",
        ));
    }
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nPragma: no-cache\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );

    stream
        .write_all(response.as_bytes())
        .map_err(|source| AppError::Io {
            context: "HTTP response write failed",
            source,
        })
}

pub fn html_escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(character),
        }
    }
    escaped
}

pub fn html_message(title: &str, message: &str) -> String {
    let title = html_escape(title);
    let message = html_escape(message);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'; img-src data:; base-uri 'none'; form-action 'none'\"><meta name=\"referrer\" content=\"no-referrer\"><title>{title}</title><style>body{{font-family:Segoe UI,Arial,sans-serif;background:#f6f8fb;color:#1d2733;padding:30px}}.card{{max-width:640px;margin:0 auto;background:white;border-radius:14px;padding:24px;box-shadow:0 10px 30px rgba(20,37,63,.08)}}h1{{margin:0 0 12px 0;font-size:22px}}p{{margin:0;font-size:15px;line-height:1.45}}</style></head><body><div class=\"card\"><h1>{title}</h1><p>{message}</p></div></body></html>"
    )
}

pub(crate) fn read_callback_request(stream: &mut TcpStream) -> AppResult<Vec<u8>> {
    let mut request = Vec::with_capacity(2048);
    let mut chunk = [0u8; 2048];

    while request.len() < MAX_CALLBACK_REQUEST_BYTES {
        let read = stream.read(&mut chunk).map_err(|source| {
            let context = if source.kind() == std::io::ErrorKind::TimedOut {
                "Callback request timed out"
            } else {
                "Failed to read callback request"
            };
            AppError::Io { context, source }
        })?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&chunk[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            return Ok(request);
        }
    }

    if request.len() >= MAX_CALLBACK_REQUEST_BYTES {
        return Err(AppError::msg(
            "OAuth callback request headers are too large",
        ));
    }
    Ok(request)
}

pub fn parse_callback_target(request: &[u8]) -> AppResult<String> {
    let request = String::from_utf8_lossy(request);
    let first_line = request.lines().next().unwrap_or_default();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let path_with_query = parts.next().unwrap_or_default();

    if !method.eq_ignore_ascii_case("GET") {
        return Err(AppError::msg("OAuth callback requires GET"));
    }
    if path_with_query.is_empty() || path_with_query.len() > MAX_CALLBACK_URL_BYTES {
        return Err(AppError::msg("OAuth callback request target is invalid"));
    }
    if path_with_query.starts_with("http://") || path_with_query.starts_with("https://") {
        return Err(AppError::msg(
            "OAuth callback request target must not be an absolute URI",
        ));
    }

    let mut host: Option<String> = None;
    let mut content_length: Option<u64> = None;
    let mut has_transfer_encoding = false;
    for line in request.lines().skip(1) {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("host") {
            host = Some(value.trim().to_string());
        } else if name.eq_ignore_ascii_case("content-length") {
            content_length = value.trim().parse().ok();
        } else if name.eq_ignore_ascii_case("transfer-encoding") {
            has_transfer_encoding = true;
        }
    }
    if !matches!(host.as_deref(), Some("localhost:1455" | "127.0.0.1:1455")) {
        return Err(AppError::msg(
            "OAuth callback request Host must be localhost:1455 or 127.0.0.1:1455",
        ));
    }

    let header_end = request
        .find("\r\n\r\n")
        .map(|index| index + 4)
        .unwrap_or(request.len());
    if content_length.is_some_and(|length| length > 0)
        || has_transfer_encoding
        || !request[header_end..].is_empty()
    {
        return Err(AppError::msg(
            "OAuth callback GET request must not have a body",
        ));
    }

    Ok(path_with_query.to_string())
}

pub fn handle_callback_stream(mut stream: TcpStream, shared: &Arc<SharedState>) -> AppResult<()> {
    let _connection = CallbackConnectionGuard::acquire()?;

    stream
        .set_read_timeout(Some(CALLBACK_IO_TIMEOUT))
        .map_err(|source| AppError::Io {
            context: "Failed to set callback read timeout",
            source,
        })?;
    stream
        .set_write_timeout(Some(CALLBACK_IO_TIMEOUT))
        .map_err(|source| AppError::Io {
            context: "Failed to set callback write timeout",
            source,
        })?;

    let request_bytes = read_callback_request(&mut stream)?;
    if request_bytes.is_empty() {
        return Err(AppError::msg("Callback request was empty"));
    }

    let path_with_query = match parse_callback_target(&request_bytes) {
        Ok(target) => target,
        Err(error) => {
            return write_error_http_response(
                &mut stream,
                "400 Bad Request",
                "Bad Request",
                &error.user_message(),
            );
        }
    };

    // A GET request has no body unless framing headers declare one (already
    // rejected above). Catch bytes that arrived just after the header packet as
    // well, without holding a callback connection for the full I/O timeout.
    stream
        .set_read_timeout(Some(Duration::from_millis(50)))
        .map_err(|source| AppError::Io {
            context: "Failed to set callback body probe timeout",
            source,
        })?;
    let mut pending = [0_u8; 1];
    let has_pending_body = match stream.peek(&mut pending) {
        Ok(read) => read > 0,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ) =>
        {
            false
        }
        Err(source) => {
            return Err(AppError::Io {
                context: "Failed to probe OAuth callback request body",
                source,
            });
        }
    };
    stream
        .set_read_timeout(Some(CALLBACK_IO_TIMEOUT))
        .map_err(|source| AppError::Io {
            context: "Failed to restore callback read timeout",
            source,
        })?;
    if has_pending_body {
        return write_error_http_response(
            &mut stream,
            "400 Bad Request",
            "OAuth callback rejected",
            "OAuth callback GET request must not have a body.",
        );
    }

    let pruned = prune_oauth_flows(shared, now_ts());
    if pruned > 0 {
        log::info!("Pruned {pruned} stale OAuth flow(s)");
    }

    let callback_url = format!("http://localhost:1455{path_with_query}");
    let parsed = match Url::parse(&callback_url) {
        Ok(url) => url,
        Err(_) => {
            return write_error_http_response(
                &mut stream,
                "400 Bad Request",
                "Invalid Request",
                "Could not parse callback URL.",
            );
        }
    };

    if parsed.path() != "/auth/callback" {
        return write_error_http_response(
            &mut stream,
            "404 Not Found",
            "Not Found",
            "This endpoint is only used for OAuth callback.",
        );
    }

    let code = parsed
        .query_pairs()
        .find(|(key, _)| key == "code")
        .map(|(_, value)| value.to_string());
    let state_value = parsed
        .query_pairs()
        .find(|(key, _)| key == "state")
        .map(|(_, value)| value.to_string());

    let Some(code) = code else {
        return write_error_http_response(
            &mut stream,
            "400 Bad Request",
            "Callback Error",
            "Query does not contain OAuth code.",
        );
    };
    let Some(state_value) = state_value else {
        return write_error_http_response(
            &mut stream,
            "400 Bad Request",
            "Callback Error",
            "Query does not contain OAuth state.",
        );
    };

    let flow_id: Option<String> = {
        let mut flows = lock_flows(shared)?;
        let mut matching_id: Option<String> = None;
        for (id, flow) in flows.iter_mut() {
            if flow.state == state_value && matches!(flow.status, OauthFlowStatus::WaitingCallback)
            {
                flow.callback_url = Some(callback_url.clone());
                flow.status = OauthFlowStatus::Exchanging;
                flow.updated_at = now_ts();
                matching_id = Some(id.clone());
                break;
            }
        }
        matching_id
    };

    let Some(flow_id) = flow_id else {
        return write_error_http_response(
            &mut stream,
            "400 Bad Request",
            "Callback Error",
            "No active OAuth flow matched this state.",
        );
    };

    match tauri::async_runtime::block_on(complete_oauth_code(
        shared,
        &flow_id,
        &code,
        Some(callback_url),
    )) {
        Ok(_) => {
            let body = html_message(
                "Login Completed",
                "OAuth completed successfully. You can return to the app now.",
            );
            write_http_response(&mut stream, "200 OK", &body)
        }
        Err(err) => {
            log::warn!("OAuth callback exchange failed: {}", err.user_message());
            write_error_http_response(
                &mut stream,
                "400 Bad Request",
                "OAuth Failed",
                "Login could not be completed. Return to the app for details and try again.",
            )
        }
    }
}

struct CallbackConnectionGuard;

impl CallbackConnectionGuard {
    fn acquire() -> AppResult<Self> {
        let mut current = ACTIVE_CALLBACK_CONNECTIONS.load(Ordering::SeqCst);
        loop {
            if current >= MAX_CALLBACK_CONNECTIONS {
                return Err(AppError::msg(
                    "Too many concurrent OAuth callback connections",
                ));
            }
            match ACTIVE_CALLBACK_CONNECTIONS.compare_exchange_weak(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Ok(Self),
                Err(actual) => current = actual,
            }
        }
    }
}

impl Drop for CallbackConnectionGuard {
    fn drop(&mut self) {
        ACTIVE_CALLBACK_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_target_rejects_non_get_absolute_uri_wrong_host_or_body() {
        let valid =
            b"GET /auth/callback?code=abc&state=xyz HTTP/1.1\r\nHost: localhost:1455\r\n\r\n";
        assert_eq!(
            parse_callback_target(valid).expect("valid target"),
            "/auth/callback?code=abc&state=xyz"
        );
        assert_eq!(
            parse_callback_target(
                b"GET /auth/callback?code=abc&state=xyz HTTP/1.1\r\nHost: 127.0.0.1:1455\r\n\r\n"
            )
            .expect("valid numeric localhost target"),
            "/auth/callback?code=abc&state=xyz"
        );

        assert!(
            parse_callback_target(b"POST /auth/callback HTTP/1.1\r\nHost: localhost:1455\r\n\r\n")
                .is_err()
        );
        assert!(
            parse_callback_target(
                b"GET http://localhost:1455/auth/callback HTTP/1.1\r\nHost: localhost:1455\r\n\r\n"
            )
            .is_err()
        );
        assert!(
            parse_callback_target(b"GET /auth/callback HTTP/1.1\r\nHost: evil.example\r\n\r\n")
                .is_err()
        );
        assert!(parse_callback_target(
            b"GET /auth/callback HTTP/1.1\r\nHost: localhost:1455\r\nContent-Length: 3\r\n\r\nabc"
        )
        .is_err());
        assert!(parse_callback_target(
            b"GET /auth/callback HTTP/1.1\r\nHost: localhost:1455\r\nTransfer-Encoding: chunked\r\n\r\n0\r\n\r\n"
        )
        .is_err());
    }

    #[test]
    fn callback_target_rejects_oversized_request_target() {
        let mut request = format!(
            "GET /auth/callback?{} HTTP/1.1\r\nHost: localhost:1455\r\n\r\n",
            "x".repeat(2 * 1024)
        )
        .into_bytes();
        request.truncate(request.len() - 1);

        assert!(parse_callback_target(&request).is_err());
    }

    #[test]
    fn html_escaping_and_security_headers_are_applied() {
        assert_eq!(
            html_escape("<script>alert(\"&'\")</script>"),
            "&lt;script&gt;alert(&quot;&amp;&#39;&quot;)&lt;/script&gt;"
        );
        let msg = html_message("Hello", "World");
        assert!(msg.contains("Content-Security-Policy"));
        assert!(msg.contains("Hello"));
        assert!(msg.contains("World"));
    }

    #[test]
    fn callback_connection_slots_are_bounded() {
        let mut guards = Vec::new();
        for _ in 0..MAX_CALLBACK_CONNECTIONS {
            guards.push(CallbackConnectionGuard::acquire().expect("slot available"));
        }
        assert!(CallbackConnectionGuard::acquire().is_err());
        assert_eq!(
            ACTIVE_CALLBACK_CONNECTIONS.load(Ordering::SeqCst),
            MAX_CALLBACK_CONNECTIONS
        );

        guards.clear();
        assert!(CallbackConnectionGuard::acquire().is_ok());
        ACTIVE_CALLBACK_CONNECTIONS.fetch_sub(1, Ordering::SeqCst);
    }

    #[test]
    fn callback_bind_failure_is_reported_synchronously() {
        let occupied = TcpListener::bind(CALLBACK_ADDR).expect("occupy callback port");

        let error = bind_callback_listener().expect_err("bind must fail fast");

        assert!(error.user_message().contains("Failed to bind"));
        drop(occupied);
    }

    #[test]
    fn callback_reader_accepts_fragmented_http_headers() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let address = listener.local_addr().expect("listener address");
        let reader = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept test connection");
            read_callback_request(&mut stream).expect("read callback request")
        });

        let mut client = TcpStream::connect(address).expect("connect test client");
        client
            .write_all(b"GET /auth/callback?code=test")
            .expect("write first request fragment");
        client
            .write_all(b"&state=test HTTP/1.1\r\nHost: localhost\r\n\r\n")
            .expect("write second request fragment");
        client
            .shutdown(std::net::Shutdown::Write)
            .expect("finish request");

        let request = reader.join().expect("join callback reader");
        assert!(request.ends_with(b"\r\n\r\n"));
        assert!(String::from_utf8_lossy(&request).contains("state=test"));
    }
}
