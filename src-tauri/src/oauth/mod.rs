pub mod server;

pub use server::{
    CALLBACK_ADDR, MAX_OAUTH_ERROR_BODY_BYTES, MAX_OAUTH_SUCCESS_BODY_BYTES, OAUTH_REDIRECT_URI,
    ensure_callback_server, handle_callback_stream, html_escape, html_message,
    parse_callback_target, write_error_http_response, write_http_response,
    write_http_response_with_limit,
};

pub use crate::providers::chatgpt::oauth::*;
