use std::sync::RwLock;

use once_cell::sync::Lazy;
use regex::Regex;

static AUTH_FIELD: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"(?i)\b(api[_-]?key|access[_-]?token|refresh[_-]?token|authorization|oauth[_-]?token|key)\b([\"'\s:=]+)([^\s,}\"]+)"#)
        .expect("valid secret-field regex")
});
static SECRET_PREFIX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(sk-[A-Za-z0-9_-]{8,}|sk-ant-[A-Za-z0-9_-]{8,}|Bearer\s+[A-Za-z0-9._~+/-]{8,})",
    )
    .expect("valid secret-prefix regex")
});

static REGISTERED_SECRETS: Lazy<RwLock<Vec<String>>> = Lazy::new(|| RwLock::new(Vec::new()));

#[derive(Debug, Clone, Default)]
pub struct Redactor {
    _private: (),
}

impl Redactor {
    pub fn register(&self, value: impl Into<String>) {
        let value = value.into();
        if value.len() < 4 {
            return;
        }
        let mut secrets = REGISTERED_SECRETS.write().expect("redactor lock poisoned");
        if !secrets.contains(&value) {
            secrets.push(value);
            secrets.sort_by_key(|value| std::cmp::Reverse(value.len()));
        }
    }

    pub fn sanitize(&self, input: impl AsRef<str>) -> String {
        sanitize_registered(input)
    }
}

pub fn sanitize_registered(input: impl AsRef<str>) -> String {
    let mut output = input.as_ref().to_string();
    for secret in REGISTERED_SECRETS
        .read()
        .expect("redactor lock poisoned")
        .iter()
    {
        output = output.replace(secret, "[REDACTED]");
    }
    output = SECRET_PREFIX
        .replace_all(&output, "[REDACTED]")
        .into_owned();
    AUTH_FIELD
        .replace_all(&output, "$1$2[REDACTED]")
        .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_and_structural_secrets_are_redacted() {
        let redactor = Redactor::default();
        redactor.register("literal-secret-value");
        let clean = redactor.sanitize(
            r#"error key=literal-secret-value authorization: Bearer abcdefghijklmnop refresh_token: tokenvalue"#,
        );
        assert!(!clean.contains("literal-secret-value"));
        assert!(!clean.contains("abcdefghijklmnop"));
        assert!(!clean.contains("tokenvalue"));
        let open_code = redactor.sanitize(r#"{"type":"api","key":"open-code-secret"}"#);
        assert!(!open_code.contains("open-code-secret"));
    }
}
