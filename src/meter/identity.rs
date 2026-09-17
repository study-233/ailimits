//! Account keys contain only a SHA-256 digest. Tokens never enter history files.
use serde_json::Value;
use std::path::PathBuf;
#[derive(Clone)]
pub struct Identity {
    pub key: String,
    pub email: Option<String>,
    pub account_id: Option<String>,
}
pub fn codex_home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"))
}
fn digest(value: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, value.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn claims(token: &str) -> Option<Value> {
    let part = token.split('.').nth(1)?;
    if part.len() > 65536 {
        return None;
    }
    let mut bytes = Vec::new();
    let mut bits = 0u32;
    let mut count = 0;
    for c in part.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => break,
            _ => return None,
        };
        bits = (bits << 6) | v as u32;
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push((bits >> count) as u8);
        }
    }
    serde_json::from_slice(&bytes).ok()
}
pub fn token_identity(token: &str) -> Identity {
    let c = claims(token).unwrap_or(Value::Null);
    let auth = &c["https://api.openai.com/auth"];
    let account = auth["chatgpt_account_id"].as_str();
    let user = auth["chatgpt_user_id"]
        .as_str()
        .or_else(|| c["sub"].as_str());
    let key = match (account, user) {
        (Some(a), Some(u)) => digest(&format!("account:{a}:user:{u}")),
        _ => digest(&format!("token:{token}")),
    };
    let email = c["https://api.openai.com/profile"]["email"]
        .as_str()
        .or_else(|| c["email"].as_str())
        .map(|s| s.trim().to_lowercase());
    Identity {
        key,
        email,
        account_id: account.map(str::to_owned),
    }
}
pub fn local_identity() -> Option<Identity> {
    let v: Value =
        serde_json::from_slice(&std::fs::read(codex_home().join("auth.json")).ok()?).ok()?;
    let mut id = token_identity(v["tokens"]["access_token"].as_str()?);
    if id.email.is_none() {
        id.email = v["tokens"]["id_token"]
            .as_str()
            .and_then(claims)
            .and_then(|c| c["email"].as_str().map(|s| s.trim().to_lowercase()));
    }
    Some(id)
}
pub fn matches_account(expected: &str, local: &Identity, account: &Value) -> bool {
    expected == local.key
        && account["type"] == "chatgpt"
        && local
            .email
            .as_deref()
            .zip(account["email"].as_str())
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b.trim()))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_survives_token_rotation_without_storing_credentials() {
        let first=token_identity("header.eyJzdWIiOiAidXNlci1maXh0dXJlIiwgImh0dHBzOi8vYXBpLm9wZW5haS5jb20vYXV0aCI6IHsiY2hhdGdwdF9hY2NvdW50X2lkIjogImFjY291bnQtZml4dHVyZSJ9LCAiZW1haWwiOiAiZml4dHVyZUBleGFtcGxlLnRlc3QifQ.signature-one");
        let second=token_identity("header.eyJzdWIiOiAidXNlci1maXh0dXJlIiwgImh0dHBzOi8vYXBpLm9wZW5haS5jb20vYXV0aCI6IHsiY2hhdGdwdF9hY2NvdW50X2lkIjogImFjY291bnQtZml4dHVyZSJ9LCAiZW1haWwiOiAiZml4dHVyZUBleGFtcGxlLnRlc3QifQ.signature-two");
        assert_eq!(first.key, second.key);
        assert_eq!(first.key.len(), 64);
        assert_eq!(first.email.as_deref(), Some("fixture@example.test"));
        assert!(!first.key.contains("fixture"));
    }
    #[test]
    fn isolate_and_match() {
        assert_ne!(token_identity("a").key, token_identity("b").key);
        let local = Identity {
            account_id: Some("account-a".into()),
            key: "a".into(),
            email: Some("a@example.test".into()),
        };
        let account = serde_json::json!({"type":"chatgpt","email":"A@example.test"});
        assert!(matches_account("a", &local, &account));
        assert!(!matches_account("b", &local, &account));
        assert!(!matches_account(
            "a",
            &Identity {
                account_id: Some("account-a".into()),
                key: "a".into(),
                email: None
            },
            &account
        ));
    }
}
