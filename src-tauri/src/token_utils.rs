use base64::Engine;
use serde_json::Value;

pub fn decode_jwt_payload(token: &str) -> Option<Value> {
    let payload = token.split('.').nth(1)?;
    let mut padded = payload.to_string();
    while padded.len() % 4 != 0 {
        padded.push('=');
    }
    let decoded = base64::engine::general_purpose::URL_SAFE
        .decode(padded.as_bytes())
        .ok()?;
    serde_json::from_slice(&decoded).ok()
}

pub fn extract_account_id(id_token: &str) -> Option<String> {
    let payload = decode_jwt_payload(id_token)?;
    let auth_claim = payload
        .get("https://api.openai.com/auth")
        .and_then(Value::as_object);

    auth_claim
        .and_then(|auth| {
            auth.get("chatgpt_account_id")
                .or_else(|| auth.get("account_id"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .or_else(|| {
            payload
                .get("sub")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
}

pub fn extract_email(id_token: &str) -> Option<String> {
    decode_jwt_payload(id_token)?
        .get("email")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

pub fn token_exp(token: &str) -> Option<i64> {
    decode_jwt_payload(token)?
        .get("exp")
        .and_then(Value::as_i64)
}
