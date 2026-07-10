use base64::Engine as _;
use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
use serde_json::Value;

const AUTH_CLAIMS_KEY: &str = "https://api.openai.com/auth";

pub(super) fn chatgpt_account_id_from_jwt(jwt: &str) -> Option<String> {
    let mut parts = jwt.split('.');
    let payload = match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(header), Some(payload), Some(signature), None)
            if !header.is_empty() && !payload.is_empty() && !signature.is_empty() =>
        {
            payload
        }
        _ => return None,
    };

    let decoded = URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| URL_SAFE.decode(payload))
        .ok()?;
    let value = serde_json::from_slice::<Value>(&decoded).ok()?;
    let auth = value
        .get(AUTH_CLAIMS_KEY)
        .and_then(Value::as_object)
        .or_else(|| value.as_object())?;
    auth.get("chatgpt_account_id")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;

    #[test]
    fn extracts_chatgpt_account_id_from_nested_auth_claims() {
        let payload = serde_json::json!({
            AUTH_CLAIMS_KEY: {
                "chatgpt_account_id": "account-123"
            }
        });
        let jwt = fake_jwt(payload);

        assert_eq!(
            chatgpt_account_id_from_jwt(&jwt),
            Some("account-123".to_string())
        );
    }

    fn fake_jwt(payload: Value) -> String {
        let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"none"}"#);
        let payload = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&payload).unwrap());
        format!("{header}.{payload}.signature")
    }
}
