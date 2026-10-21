//! Where a source's token comes from: `FT_<NAME>_TOKEN`, then (GitHub only)
//! `gh auth token`, then the OS keychain (service `ft`, account
//! `<name>:<email or "github">`). No token is a normal state — the source is
//! skipped.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::http::Secret;
use crate::config::{SourceCfg, SourceKind};

const KEYCHAIN_SERVICE: &str = "ft";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredOrigin {
    Env,
    Gh,
    Keychain,
}

impl CredOrigin {
    pub fn as_str(self) -> &'static str {
        match self {
            CredOrigin::Env => "env",
            CredOrigin::Gh => "gh",
            CredOrigin::Keychain => "keychain",
        }
    }
}

/// `FT_<NAME>_TOKEN` for a source name (upper-cased, `-` → `_`).
pub fn env_var_name(name: &str) -> String {
    format!("FT_{}_TOKEN", name.to_uppercase().replace('-', "_"))
}

/// Keychain account for a source.
pub fn keychain_account(name: &str, cfg: &SourceCfg) -> String {
    let account = match cfg.kind {
        SourceKind::Jira => cfg.email.as_deref().unwrap_or(""),
        SourceKind::Github => "github",
    };
    format!("{name}:{account}")
}

/// Resolve a token from the real environment, `gh`, and keychain.
pub fn resolve(name: &str, cfg: &SourceCfg) -> Option<(Secret, CredOrigin)> {
    resolve_with(
        name,
        cfg,
        |var| std::env::var(var).ok(),
        gh_auth_token,
        keychain_get,
    )
}

/// [`resolve`] with injectable lookups (tests).
pub fn resolve_with(
    name: &str,
    cfg: &SourceCfg,
    env: impl Fn(&str) -> Option<String>,
    gh: impl Fn() -> Option<String>,
    keychain: impl Fn(&str) -> Option<String>,
) -> Option<(Secret, CredOrigin)> {
    let non_empty = |s: String| (!s.trim().is_empty()).then(|| s.trim().to_string());
    if let Some(t) = env(&env_var_name(name)).and_then(non_empty) {
        return Some((Secret::new(t), CredOrigin::Env));
    }
    if cfg.kind == SourceKind::Github {
        if let Some(t) = gh().and_then(non_empty) {
            return Some((Secret::new(t), CredOrigin::Gh));
        }
    }
    keychain(&keychain_account(name, cfg))
        .and_then(non_empty)
        .map(|t| (Secret::new(t), CredOrigin::Keychain))
}

fn keychain_get(account: &str) -> Option<String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, account)
        .ok()?
        .get_password()
        .ok()
}

/// Store a token in the keychain.
pub fn keychain_store(name: &str, cfg: &SourceCfg, token: &Secret) -> Result<(), String> {
    keyring::Entry::new(KEYCHAIN_SERVICE, &keychain_account(name, cfg))
        .and_then(|e| e.set_password(token.expose()))
        .map_err(|e| format!("keychain: {e}"))
}

/// Delete a stored token. Deleting a missing entry is not an error.
pub fn keychain_delete(name: &str, cfg: &SourceCfg) -> Result<(), String> {
    let entry = keyring::Entry::new(KEYCHAIN_SERVICE, &keychain_account(name, cfg))
        .map_err(|e| format!("keychain: {e}"))?;
    match entry.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("keychain: {e}")),
    }
}

/// `gh auth token`, if `gh` is installed and logged in (5 s cap).
fn gh_auth_token() -> Option<String> {
    let mut child = Command::new("gh")
        .args(["auth", "token"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait().ok()? {
            Some(status) if status.success() => break,
            Some(_) => return None,
            None if Instant::now() > deadline => {
                let _ = child.kill();
                return None;
            }
            None => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(kind: &str) -> SourceCfg {
        toml::from_str(&format!(
            "kind = \"{kind}\"\nemail = \"fake-user@example.test\"\nbase_url = \"https://example.test\"\n"
        ))
        .unwrap()
    }

    #[test]
    fn env_name_normalisation() {
        assert_eq!(env_var_name("jira"), "FT_JIRA_TOKEN");
        assert_eq!(env_var_name("my-jira"), "FT_MY_JIRA_TOKEN");
    }

    #[test]
    fn precedence_env_then_gh_then_keychain() {
        // NOT VALID — test data only.
        let env = |v: &str| (v == "FT_GH_TOKEN").then(|| "FAKE_TEST_env".to_string());
        let gh = || Some("FAKE_TEST_gh".to_string());
        let kc = |_: &str| Some("FAKE_TEST_kc".to_string());
        let (s, o) = resolve_with("gh", &cfg("github"), env, gh, kc).unwrap();
        assert_eq!((s.expose(), o), ("FAKE_TEST_env", CredOrigin::Env));
        let (s, o) = resolve_with("gh", &cfg("github"), |_| None, gh, kc).unwrap();
        assert_eq!((s.expose(), o), ("FAKE_TEST_gh", CredOrigin::Gh));
        // Jira never asks gh.
        let (s, o) = resolve_with("jira", &cfg("jira"), |_| None, gh, kc).unwrap();
        assert_eq!((s.expose(), o), ("FAKE_TEST_kc", CredOrigin::Keychain));
        assert!(resolve_with(
            "jira",
            &cfg("jira"),
            |_| Some("  ".into()),
            || None,
            |_| None
        )
        .is_none());
    }

    #[test]
    fn keychain_accounts() {
        assert_eq!(
            keychain_account("jira", &cfg("jira")),
            "jira:fake-user@example.test"
        );
        assert_eq!(keychain_account("gh", &cfg("github")), "gh:github");
    }
}
