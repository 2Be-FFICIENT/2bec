//! Local inspection of API credentials.
//!
//! The API accepts two bearer tokens: an API key shaped
//! `2bef_<env>_<32 alphanumerics>` minted in the web app, and a person's
//! OAuth access token (a JWT). Only the key format is checked locally; any
//! other value is sent as is and left to the server to judge.

use std::fmt;

/// Prefix shared by every API key.
pub const KEY_PREFIX: &str = "2bef_";

/// Length of the random part of an API key.
const KEY_SECRET_LEN: usize = 32;

/// Which environment minted an API key. The server does not enforce it; it
/// only tells the user which API URL the key is meant for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyEnv {
    Live,
    Next,
    Dev,
}

impl KeyEnv {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "live" => Some(Self::Live),
            "next" => Some(Self::Next),
            "dev" => Some(Self::Dev),
            _ => None,
        }
    }
}

impl fmt::Display for KeyEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Live => "live",
            Self::Next => "next",
            Self::Dev => "dev",
        })
    }
}

/// What kind of credential a token string is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    /// A well-formed API key minted for `env`.
    ApiKey { env: KeyEnv },
    /// Anything not starting with `2bef_`: treated as an OAuth access token.
    Oauth,
}

/// Why a `2bef_` token is not a valid API key.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TokenError {
    #[error("API key environment `{0}` is not one of live, next, dev")]
    UnknownEnv(String),
    #[error("API key must be `2bef_<env>_<{KEY_SECRET_LEN} letters or digits>`")]
    Malformed,
}

/// Classify `token`, rejecting malformed API keys before any network call.
pub fn classify(token: &str) -> Result<TokenKind, TokenError> {
    let Some(rest) = token.strip_prefix(KEY_PREFIX) else {
        return Ok(TokenKind::Oauth);
    };
    let Some((env, secret)) = rest.split_once('_') else {
        return Err(TokenError::Malformed);
    };
    let env = KeyEnv::parse(env).ok_or_else(|| TokenError::UnknownEnv(env.to_string()))?;
    if secret.len() != KEY_SECRET_LEN || !secret.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(TokenError::Malformed);
    }
    Ok(TokenKind::ApiKey { env })
}

/// The display prefix of a token, safe to print: the first 12 characters of
/// an API key (`2bef_live_ab`), or a fixed placeholder for an OAuth token.
pub fn display_prefix(token: &str) -> String {
    if token.starts_with(KEY_PREFIX) {
        let end = token.char_indices().nth(12).map_or(token.len(), |(i, _)| i);
        format!("{}…", &token[..end])
    } else {
        "<oauth token>".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "abcdefghijklmnopqrstuvwxyz012345";

    #[test]
    fn well_formed_keys_parse_with_their_env() {
        for (env, want) in [
            ("live", KeyEnv::Live),
            ("next", KeyEnv::Next),
            ("dev", KeyEnv::Dev),
        ] {
            let k = classify(&format!("2bef_{env}_{SECRET}")).unwrap();
            assert_eq!(k, TokenKind::ApiKey { env: want });
        }
    }

    #[test]
    fn non_prefixed_tokens_are_oauth() {
        assert_eq!(
            classify("eyJhbGciOi.payload.sig").unwrap(),
            TokenKind::Oauth
        );
        assert_eq!(classify("").unwrap(), TokenKind::Oauth);
    }

    #[test]
    fn unknown_env_is_rejected() {
        let err = classify(&format!("2bef_prod_{SECRET}")).unwrap_err();
        assert_eq!(err, TokenError::UnknownEnv("prod".into()));
    }

    #[test]
    fn wrong_secret_length_or_charset_is_rejected() {
        assert_eq!(
            classify("2bef_live_short").unwrap_err(),
            TokenError::Malformed
        );
        assert_eq!(
            classify(&format!("2bef_live_{SECRET}0")).unwrap_err(),
            TokenError::Malformed
        );
        let bad = format!("2bef_live_{}-", &SECRET[..31]);
        assert_eq!(classify(&bad).unwrap_err(), TokenError::Malformed);
        assert_eq!(classify("2bef_live").unwrap_err(), TokenError::Malformed);
    }

    #[test]
    fn display_prefix_hides_the_secret() {
        assert_eq!(
            display_prefix(&format!("2bef_live_{SECRET}")),
            "2bef_live_ab…"
        );
        assert_eq!(display_prefix("2bef_"), "2bef_…");
        assert_eq!(display_prefix("eyJ.x.y"), "<oauth token>");
    }
}
