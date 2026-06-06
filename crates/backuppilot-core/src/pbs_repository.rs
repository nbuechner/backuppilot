//! PBS connection fields for profiles and `proxmox-backup-client`.
//!
//! Profiles store credentials as JSON (`to_storage`) so UI values round-trip exactly.
//! Legacy one-line format `{user}@{host}!{token}@{datastore}` is still read for old DB rows.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PbsCredentialsJson {
    user: String,
    host: String,
    token: String,
    datastore: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PbsRepositoryParts {
    pub user: String,
    pub host: String,
    pub token: String,
    pub datastore: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepositoryParseError {
    Empty,
    MissingDatastore,
    MissingHost,
}

impl std::fmt::Display for RepositoryParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "repository string is empty"),
            Self::MissingDatastore => write!(f, "missing datastore after '@'"),
            Self::MissingHost => write!(f, "missing PBS hostname"),
        }
    }
}

/// Strips optional `https://` / `http://` and paths from a hostname field.
pub fn normalize_pbs_host(host: &str) -> String {
    let mut host = host.trim();
    for prefix in ["https://", "http://"] {
        if let Some(rest) = host.strip_prefix(prefix) {
            host = rest;
            break;
        }
    }
    host.split('/').next().unwrap_or(host).trim().to_string()
}

impl PbsRepositoryParts {
    /// Decodes profile `repository` column (JSON or legacy one-line format).
    pub fn parse(repository: &str) -> Result<Self, RepositoryParseError> {
        decode_repository(repository)
    }

    /// JSON for the database — preserves exact UI field values (tokens may contain `@` and `!`).
    pub fn to_storage(&self) -> String {
        encode_repository(self)
    }
}

/// Stores PBS fields as JSON in `profiles.repository`.
pub fn encode_repository(parts: &PbsRepositoryParts) -> String {
    serde_json::to_string(&PbsCredentialsJson {
        user: parts.user.clone(),
        host: parts.host.clone(),
        token: parts.token.clone(),
        datastore: parts.datastore.clone(),
    })
    .expect("pbs credentials serialize")
}

/// One-line `user@host!token@datastore` form (used in YAML export/import).
pub fn format_legacy_repository(parts: &PbsRepositoryParts) -> String {
    format!(
        "{}@{}!{}@{}",
        parts.user.trim(),
        parts.host.trim(),
        parts.token.trim(),
        parts.datastore.trim()
    )
}

/// Reads JSON storage or legacy `user@host!token@datastore` strings.
pub fn decode_repository(repository: &str) -> Result<PbsRepositoryParts, RepositoryParseError> {
    let repository = repository.trim();
    if repository.is_empty() {
        return Err(RepositoryParseError::Empty);
    }

    if repository.starts_with('{') {
        let j: PbsCredentialsJson =
            serde_json::from_str(repository).map_err(|_| RepositoryParseError::Empty)?;
        if j.host.is_empty() {
            return Err(RepositoryParseError::MissingHost);
        }
        if j.datastore.is_empty() {
            return Err(RepositoryParseError::MissingDatastore);
        }
        return Ok(PbsRepositoryParts {
            user: j.user,
            host: j.host,
            token: j.token,
            datastore: j.datastore,
        });
    }

    parse_legacy_repository(repository)
}

fn parse_legacy_repository(repository: &str) -> Result<PbsRepositoryParts, RepositoryParseError> {
    let (rest, datastore) = repository
        .rsplit_once('@')
        .filter(|(_, ds)| !ds.is_empty())
        .ok_or(RepositoryParseError::MissingDatastore)?;

    let (user_host, token) = match rest.split_once('!') {
        Some((uh, t)) if !t.is_empty() => (uh, t),
        _ => return Err(RepositoryParseError::MissingDatastore),
    };

    let (user, host) = split_user_and_host(user_host);
    if host.is_empty() {
        return Err(RepositoryParseError::MissingHost);
    }

    Ok(PbsRepositoryParts {
        user,
        host,
        token: token.to_string(),
        datastore: datastore.to_string(),
    })
}

impl PbsRepositoryParts {

    pub fn host_for_reachability(&self) -> &str {
        self.host.split(':').next().unwrap_or(&self.host)
    }

    /// `host:port` for TCP reachability (respects custom port and IPv6).
    pub fn tcp_connect_address(&self) -> String {
        let host = normalize_pbs_host(&self.host);
        let (server, port) = parse_host_port(&host);
        format!("{server}:{port}")
    }

    /// PBS 4.x `--auth-id` (primary mode for this profile).
    pub fn auth_id(&self) -> String {
        self.auth_id_candidates()
            .into_iter()
            .next()
            .map(|(_, id)| id)
            .unwrap_or_else(|| self.user.clone())
    }

    /// API token secret — value for `PBS_PASSWORD` (UUID from PBS display token).
    pub fn api_token_secret(&self) -> String {
        if let Some((_, secret)) = try_parse_pbs_display_token(&self.token) {
            return secret;
        }
        let (token_id, secret) = self.api_token_parts();
        if !secret.is_empty() {
            return secret;
        }
        if self.user.contains('!') && token_id.is_empty() {
            return self.token.trim().to_string();
        }
        secret
    }

    /// PBS user with realm (`pbs-ch-01@pbs` if only `pbs-ch-01` was entered).
    pub fn pbs_user(&self) -> String {
        ensure_pbs_user(&self.user)
    }

    /// Full auth-id for the client (`user@pbs!tokenname`, or `user` if it already contains `!`).
    pub fn pbs_auth_id(&self) -> Option<String> {
        let user = self.user.trim();
        if user.contains('!') {
            return Some(user.to_string());
        }
        let token_id = self.token_id_verify_candidates().into_iter().next()?;
        Some(format!("{}!{}", self.pbs_user(), token_id))
    }

    /// Like [OneSystems backup.sh](https://git.onesystems.ch/system-tools/proxmox-backup-client): \
    /// `${PBS_USERNAME}@${PBS_HOST}:${PBS_PORT}:${PBS_DATASTORE}`.
    pub fn pbs_repository_bash_style(&self, auth_id: &str) -> String {
        let (server, port) = self.server_and_port();
        format!("{auth_id}@{server}:{port}:{}", self.datastore)
    }

    /// PBS 4.x `--repository` (same layout as `pbs_repository_bash_style`).
    pub fn pbs_repository_cli_arg(&self, auth_id: &str) -> String {
        self.pbs_repository_bash_style(auth_id)
    }

    /// Token names to try when verifying (explicit id from `tokenname=secret`, else UUID as id).
    pub fn token_id_verify_candidates(&self) -> Vec<String> {
        let user = self.pbs_user();
        let mut names = Vec::new();

        if let Some((token_id, _)) = try_parse_pbs_display_token(&self.token) {
            if auth_id_fits(&user, &token_id) {
                names.push(token_id);
                return names;
            }
        }

        let (token_id, secret) = self.api_token_parts();
        if is_valid_token_id(&token_id) && auth_id_fits(&user, &token_id) {
            names.push(token_id);
        } else if let Some(name) = extract_token_name_from_paste(&self.token) {
            if auth_id_fits(&user, &name) {
                names.push(name);
            }
        }

        if names.is_empty() && is_likely_api_token_secret(&secret) && auth_id_fits(&user, &secret) {
            names.push(secret);
        }

        names
    }

    /// `--repository` strings for `proxmox-backup-client` (bash script style first).
    pub fn repository_cli_candidates(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(auth_id) = self.pbs_auth_id() {
            out.push((
                "bash-repository",
                self.pbs_repository_bash_style(&auth_id),
            ));
        }
        for token_id in self.token_id_verify_candidates() {
            let auth_id = format!("{}!{}", self.pbs_user(), token_id);
            let repo = self.pbs_repository_bash_style(&auth_id);
            if out.iter().all(|(_, r)| r != &repo) {
                out.push(("repository", repo));
            }
        }
        out
    }

    /// Auth-ID variants for component flags (`--auth-id`, `--server`, …).
    pub fn auth_id_candidates(&self) -> Vec<(&'static str, String)> {
        let mut out = Vec::new();
        if let Some(auth_id) = self.pbs_auth_id() {
            out.push(("auth-id", auth_id));
        }
        for token_id in self.token_id_verify_candidates() {
            let auth_id = format!("{}!{}", self.pbs_user(), token_id);
            if out.iter().all(|(_, a)| a != &auth_id) {
                out.push(("auth-id", auth_id));
            }
        }
        out
    }

    /// Host for `--server` and port for `--port` (default 8007).
    pub fn server_and_port(&self) -> (String, u16) {
        parse_host_port(&self.host)
    }

    /// `(token_id, secret)` — `tokenid=secret`, UUID only, or legacy malformed values.
    pub fn api_token_parts(&self) -> (String, String) {
        parse_token_storage(&self.token)
    }
}

/// PBS limits `auth-id` to 64 characters (`user@realm!tokenname`).
pub const PBS_AUTH_ID_MAX_LEN: usize = 64;

/// Appends `@pbs` when the username has no realm (e.g. `pbs-ch-01-whmcs` → `pbs-ch-01-whmcs@pbs`).
pub fn ensure_pbs_user(user: &str) -> String {
    let user = user.trim();
    if user.is_empty() {
        return String::new();
    }
    if user.contains('@') {
        user.to_string()
    } else {
        format!("{user}@pbs")
    }
}

/// Realm suffix from `user@pbs` (defaults to `pbs`).
pub fn pbs_realm_from_user(user: &str) -> &str {
    user.rsplit_once('@').map(|(_, realm)| realm).unwrap_or("pbs")
}

/// PBS one-time copy format: `153-Test@pbs!432ce2bd-…` → (`153-Test`, uuid).
pub fn try_parse_pbs_display_token(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    let (left, secret) = s.rsplit_once('!')?;
    let secret = secret.trim();
    if !is_likely_api_token_secret(secret) {
        return None;
    }
    let left = left.trim();
    let (token_name, realm) = left.rsplit_once('@')?;

    // Standard PBS: `tokenname@pbs!secret` or `tokenname@pam!secret`
    if (realm == "pbs" || realm == "pam") && is_valid_token_id(token_name) {
        return Some((token_name.to_string(), secret.to_string()));
    }

    // Legacy/corrupted saves merged hostname into the token (e.g. `153-Test@pbs-host.example.ch!uuid`)
    if is_valid_token_id(token_name) {
        return Some((token_name.to_string(), secret.to_string()));
    }

    None
}

/// Token ID from PBS (not the secret UUID); must not contain `@`, `:`, or `!`.
pub fn is_valid_token_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && !id.contains(['@', ':', '!', '='])
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// API token secret (typically the UUID PBS shows once).
pub fn is_likely_api_token_secret(s: &str) -> bool {
    let s = s.trim();
    if s.is_empty() || s.contains(['@', ':', '!']) {
        return false;
    }
    s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

pub fn auth_id_fits(user: &str, token_id: &str) -> bool {
    user.len() + 1 + token_id.len() <= PBS_AUTH_ID_MAX_LEN
}

/// Extracts token name from pasted `user@pbs!name@host:…` or `user@pbs!name=secret`.
pub fn extract_token_name_from_paste(s: &str) -> Option<String> {
    let after_bang = s.rsplit_once('!')?.1;
    let name = after_bang
        .split(['@', '=', ':'])
        .next()?
        .trim();
    is_valid_token_id(name).then(|| name.to_string())
}

fn parse_token_storage(token: &str) -> (String, String) {
    let token = token.trim();
    if token.is_empty() {
        return (String::new(), String::new());
    }

    if let Some((name, secret)) = try_parse_pbs_display_token(token) {
        return (name, secret);
    }

    if let Some(p) = try_parse_pbs_cli_repository(token) {
        return (p.token, String::new());
    }

    if let Some(eq_pos) = token.rfind('=') {
        let (id, secret) = token.split_at(eq_pos);
        let id = id.trim();
        let secret = secret.trim_start_matches('=').trim();
        if is_valid_token_id(id) && is_likely_api_token_secret(secret) {
            return (id.to_string(), secret.to_string());
        }
        if let Some(name) = extract_token_name_from_paste(token) {
            if is_likely_api_token_secret(secret) {
                return (name, secret.to_string());
            }
        }
    }

    // A stored token name (e.g. "win-test") is a valid token_id. Check before
    // is_likely_api_token_secret, which also matches short names like "win-test".
    // Real PBS secrets are UUIDs (36 chars) and fail is_valid_token_id's length check.
    if is_valid_token_id(token) {
        return (token.to_string(), String::new());
    }

    if is_likely_api_token_secret(token) {
        return (String::new(), token.to_string());
    }

    if let Some(name) = extract_token_name_from_paste(token) {
        return (name, String::new());
    }

    (String::new(), token.to_string())
}

/// Splits `host`, `host:port`, or `[ipv6]:port` for proxmox-backup-client flags.
fn parse_host_port(host: &str) -> (String, u16) {
    const DEFAULT_PORT: u16 = 8007;

    if let Some(rest) = host.strip_prefix('[') {
        if let Some((addr, port_str)) = rest.split_once("]:") {
            let port = port_str.parse().unwrap_or(DEFAULT_PORT);
            return (format!("[{addr}]"), port);
        }
        return (host.to_string(), DEFAULT_PORT);
    }

    if let Some((h, port_str)) = host.rsplit_once(':') {
        if let Ok(port) = port_str.parse::<u16>() {
            return (h.to_string(), port);
        }
    }

    (host.to_string(), DEFAULT_PORT)
}

/// Parses PBS UI connection string `user@pbs!token@host[:port]:datastore`.
pub fn try_parse_pbs_cli_repository(s: &str) -> Option<PbsRepositoryParts> {
    let s = normalize_pbs_host(s);
    if !s.contains('!') {
        return None;
    }

    let (head, datastore) = s.rsplit_once(':')?;
    if datastore.is_empty() || datastore.contains('@') || datastore.contains('!') {
        return None;
    }

    let (auth, host) = if let Some((head2, port_str)) = head.rsplit_once(':') {
        if port_str.parse::<u16>().is_ok() {
            let (auth, server) = head2.rsplit_once('@')?;
            (auth, format!("{server}:{port_str}"))
        } else {
            let (auth, server) = head.rsplit_once('@')?;
            (auth, server.to_string())
        }
    } else {
        let (auth, server) = head.rsplit_once('@')?;
        (auth, server.to_string())
    };

    let (user, token_name) = auth.split_once('!')?;
    if user.is_empty() || token_name.is_empty() || host.is_empty() {
        return None;
    }

    Some(PbsRepositoryParts {
        user: user.to_string(),
        host,
        token: token_name.to_string(),
        datastore: datastore.to_string(),
    })
}

/// Normalizes for storage: `tokenname=secret` (from PBS `tokenname@pbs!secret`, `tokenname=secret`, or UUID).
pub fn normalize_api_token_field(input: &str) -> String {
    let input = input.trim();
    if input.is_empty() {
        return String::new();
    }

    if let Some((name, secret)) = try_parse_pbs_display_token(input) {
        return format!("{name}={secret}");
    }

    if let Some(p) = try_parse_pbs_cli_repository(input) {
        return p.token;
    }

    if let Some(eq_pos) = input.rfind('=') {
        let (id, secret) = input.split_at(eq_pos);
        let id = id.trim();
        let secret = secret.trim_start_matches('=').trim();
        if is_valid_token_id(id) && is_likely_api_token_secret(secret) {
            return format!("{id}={secret}");
        }
        // `backup@pbs!mytoken=uuid` (auth-id + secret in one field)
        if let Some((_, token_name)) = id.rsplit_once('!') {
            if is_valid_token_id(token_name) && is_likely_api_token_secret(secret) {
                return format!("{token_name}={secret}");
            }
        }
    }

    if is_likely_api_token_secret(input) {
        return input.to_string();
    }

    input.to_string()
}

/// Splits `user@realm@host.example.com` into (`user@realm`, `host.example.com`).
fn split_user_and_host(user_host: &str) -> (String, String) {
    if let Some((user, host)) = user_host.rsplit_once('@') {
        (user.to_string(), host.to_string())
    } else {
        (String::new(), user_host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format_legacy(parts: &PbsRepositoryParts) -> String {
        format!(
            "{}@{}!{}@{}",
            parts.user.trim(),
            parts.host.trim(),
            parts.token.trim(),
            parts.datastore.trim()
        )
    }

    #[test]
    fn parses_standard_repository() {
        let p = PbsRepositoryParts::parse("pbs-user@pbs.example.ch!mytoken@datastore").unwrap();
        assert_eq!(p.user, "pbs-user");
        assert_eq!(p.host, "pbs.example.ch");
        assert_eq!(p.token, "mytoken");
        assert_eq!(p.datastore, "datastore");
    }

    #[test]
    fn parses_realm_username_and_port() {
        let p =
            PbsRepositoryParts::parse("root@pam@pbs.local:8007!backuppilot=secret-uuid@store1")
                .unwrap();
        assert_eq!(p.user, "root@pam");
        assert_eq!(p.host, "pbs.local:8007");
        assert_eq!(p.token, "backuppilot=secret-uuid");
        assert_eq!(p.datastore, "store1");
        assert_eq!(p.host_for_reachability(), "pbs.local");
    }

    #[test]
    fn legacy_roundtrip_simple_token() {
        let raw = "backup@pbs.example.com!token@ds";
        let p = PbsRepositoryParts::parse(raw).unwrap();
        assert_eq!(format_legacy(&p), raw);
    }

    #[test]
    fn json_storage_roundtrip_preserves_pbs_display_token() {
        let parts = PbsRepositoryParts {
            user: "pbs-ch-01-whmcs".into(),
            host: "pbs-ch-01.onesystems.ch".into(),
            token: "153-Test@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396".into(),
            datastore: "DS01".into(),
        };
        let stored = parts.to_storage();
        assert!(stored.starts_with('{'));
        let back = PbsRepositoryParts::parse(&stored).unwrap();
        assert_eq!(back.user, parts.user);
        assert_eq!(back.host, parts.host);
        assert_eq!(back.token, parts.token);
        assert_eq!(back.datastore, parts.datastore);
    }

    #[test]
    fn legacy_parse_does_not_corrupt_token_with_bang() {
        let parts = PbsRepositoryParts {
            user: "pbs-ch-01-whmcs".into(),
            host: "pbs-ch-01.onesystems.ch".into(),
            token: "153-Test@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396".into(),
            datastore: "DS01".into(),
        };
        let legacy = format_legacy(&parts);
        let parsed = PbsRepositoryParts::parse(&legacy).unwrap();
        assert_eq!(parsed.token, parts.token);
    }

    #[test]
    fn auth_id_and_secret_from_api_token() {
        let p =
            PbsRepositoryParts::parse("root@pam@pbs.local:8007!backuppilot=secret-uuid@store1")
                .unwrap();
        assert_eq!(p.api_token_parts(), ("backuppilot".into(), "secret-uuid".into()));
        assert_eq!(p.auth_id(), "root@pam!backuppilot");
        assert_eq!(p.api_token_secret(), "secret-uuid");
        assert_eq!(p.server_and_port(), ("pbs.local".to_string(), 8007));
    }

    #[test]
    fn parses_ipv6_host_with_port() {
        assert_eq!(
            parse_host_port("[ff80::51]:1234"),
            ("[ff80::51]".to_string(), 1234)
        );
    }

    #[test]
    fn tcp_connect_address_uses_custom_port() {
        let p = PbsRepositoryParts::parse("u@backup.example.com:8443!t@ds").unwrap();
        assert_eq!(p.tcp_connect_address(), "backup.example.com:8443");
    }

    #[test]
    fn tcp_connect_address_defaults_to_8007() {
        let p = PbsRepositoryParts::parse("u@backup.example.com!t@ds").unwrap();
        assert_eq!(p.tcp_connect_address(), "backup.example.com:8007");
    }

    #[test]
    fn api_token_parts_from_secret_only_repository() {
        let p = PbsRepositoryParts::parse(
            "u@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396@ds",
        )
        .unwrap();
        assert_eq!(
            p.api_token_parts(),
            (String::new(), "432ce2bd-1bad-4b4a-ae25-9aa33e4e3396".into())
        );
    }

    #[test]
    fn auth_id_candidates_for_secret_only_token() {
        let p = PbsRepositoryParts::parse(
            "backup@pbs@host!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396@ds",
        )
        .unwrap();
        let modes: Vec<_> = p
            .auth_id_candidates()
            .into_iter()
            .map(|(m, _)| m)
            .collect();
        assert_eq!(modes, vec!["auth-id"]);
        assert_eq!(p.auth_id(), "backup@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396");
    }

    #[test]
    fn parses_pbs_cli_connection_string() {
        let p = try_parse_pbs_cli_repository("backup@pbs!mytoken@pbs.example.com:8007:store1")
            .unwrap();
        assert_eq!(p.user, "backup@pbs");
        assert_eq!(p.host, "pbs.example.com:8007");
        assert_eq!(p.token, "mytoken");
        assert_eq!(p.datastore, "store1");
    }

    #[test]
    fn parses_pbs_cli_connection_without_port() {
        let p = try_parse_pbs_cli_repository("user@pbs!token@host:store").unwrap();
        assert_eq!(p.host, "host");
        assert_eq!(p.datastore, "store");
    }

    #[test]
    fn repository_cli_arg_matches_pbs_docs() {
        let p = PbsRepositoryParts::parse("root@pam@pbs.local:8007!backuppilot=secret@store1")
            .unwrap();
        assert_eq!(
            p.pbs_repository_cli_arg("root@pam!backuppilot"),
            "root@pam!backuppilot@pbs.local:8007:store1"
        );
    }

    #[test]
    fn bash_style_repository_matches_onesystems_backup_sh() {
        let parts = PbsRepositoryParts {
            user: "pbs-ch-01-whmcs".into(),
            host: "pbs-ch-01.onesystems.ch".into(),
            token: "153-Test@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396".into(),
            datastore: "DS01".into(),
        };
        let auth_id = parts.pbs_auth_id().unwrap();
        assert_eq!(auth_id, "pbs-ch-01-whmcs@pbs!153-Test");
        assert_eq!(
            parts.pbs_repository_bash_style(&auth_id),
            "pbs-ch-01-whmcs@pbs!153-Test@pbs-ch-01.onesystems.ch:8007:DS01"
        );
        assert_eq!(parts.api_token_secret(), "432ce2bd-1bad-4b4a-ae25-9aa33e4e3396");
    }

    #[test]
    fn normalize_api_token_field_accepts_full_token() {
        assert_eq!(
            normalize_api_token_field("backup@pbs!mytoken=432ce2bd-1bad-4b4a-ae25-9aa33e4e3396"),
            "mytoken=432ce2bd-1bad-4b4a-ae25-9aa33e4e3396"
        );
    }

    #[test]
    fn normalize_pbs_display_token_from_ui() {
        assert_eq!(
            normalize_api_token_field("153-Test@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396"),
            "153-Test=432ce2bd-1bad-4b4a-ae25-9aa33e4e3396"
        );
    }

    #[test]
    fn auth_id_for_pbs_display_token_profile() {
        let p = PbsRepositoryParts::parse(
            "pbs-ch-01-whmcs@pbs!153-Test@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396@DS01",
        )
        .unwrap();
        assert_eq!(p.user, "pbs-ch-01-whmcs");
        assert_eq!(p.host, "pbs");
        assert_eq!(p.token, "153-Test@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396");
        assert_eq!(p.pbs_user(), "pbs-ch-01-whmcs@pbs");
        assert_eq!(p.auth_id(), "pbs-ch-01-whmcs@pbs!153-Test");
    }

    #[test]
    fn parses_display_token_when_hostname_was_merged_into_realm() {
        let (id, secret) =
            try_parse_pbs_display_token("153-Test@pbs-ch-01.onesystems.ch!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396")
                .unwrap();
        assert_eq!(id, "153-Test");
        assert_eq!(secret, "432ce2bd-1bad-4b4a-ae25-9aa33e4e3396");
        let parts = PbsRepositoryParts {
            user: "pbs-ch-01-whmcs".into(),
            host: "pbs-ch-01.onesystems.ch".into(),
            token: "153-Test@pbs-ch-01.onesystems.ch!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396".into(),
            datastore: "DS01".into(),
        };
        assert!(!parts.token_id_verify_candidates().is_empty());
    }

    #[test]
    fn json_profile_builds_auth_candidates() {
        let parts = PbsRepositoryParts {
            user: "pbs-ch-01-whmcs".into(),
            host: "pbs".into(),
            token: "153-Test@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396".into(),
            datastore: "DS01".into(),
        };
        let decoded = PbsRepositoryParts::parse(&parts.to_storage()).unwrap();
        assert_eq!(decoded.token_id_verify_candidates(), vec!["153-Test".to_string()]);
        assert_eq!(decoded.auth_id(), "pbs-ch-01-whmcs@pbs!153-Test");
    }

    #[test]
    fn roundtrip_preserves_ui_fields() {
        let parts = PbsRepositoryParts {
            user: "pbs-ch-01-whmcs".into(),
            host: "pbs".into(),
            token: "153-Test@pbs!432ce2bd-1bad-4b4a-ae25-9aa33e4e3396".into(),
            datastore: "DS01".into(),
        };
        let p = PbsRepositoryParts::parse(&parts.to_storage()).unwrap();
        assert_eq!(p.user, parts.user);
        assert_eq!(p.host, parts.host);
        assert_eq!(p.token, parts.token);
        assert_eq!(p.datastore, parts.datastore);
    }

    #[test]
    fn ensure_pbs_user_appends_realm() {
        assert_eq!(ensure_pbs_user("pbs-ch-01-whmcs"), "pbs-ch-01-whmcs@pbs");
        assert_eq!(ensure_pbs_user("backup@pbs"), "backup@pbs");
    }

    #[test]
    fn api_token_parts_rejects_user_prefix_before_equals() {
        let (id, secret) = parse_token_storage("backup@pbs!mytoken=432ce2bd-1bad-4b4a-ae25-9aa33e4e3396");
        assert_eq!(id, "mytoken");
        assert_eq!(secret, "432ce2bd-1bad-4b4a-ae25-9aa33e4e3396");
    }

    #[test]
    fn pasted_connection_string_in_token_yields_short_auth_id() {
        let p = PbsRepositoryParts::parse(
            "backup@pbs@host!backup@pbs!mytoken@pbs.example.com:8007:store1@ds",
        )
        .unwrap();
        let candidates = p.auth_id_candidates();
        assert!(!candidates.is_empty());
        for (_, auth_id) in candidates {
            assert!(
                auth_id.len() <= PBS_AUTH_ID_MAX_LEN,
                "auth-id too long: {auth_id}"
            );
        }
    }

    #[test]
    fn normalize_strips_pasted_connection_to_token_name() {
        assert_eq!(
            normalize_api_token_field("backup@pbs!mytoken@pbs.example.com:8007:store1"),
            "mytoken"
        );
    }

    #[test]
    fn normalize_host_strips_scheme() {
        assert_eq!(
            normalize_pbs_host("https://pbs.example.com:8007/"),
            "pbs.example.com:8007"
        );
    }

    #[test]
    fn tcp_connect_address_strips_https_from_host_field() {
        let parts = PbsRepositoryParts {
            user: "u".into(),
            host: "https://pbs.example.com:8007".into(),
            token: "t=s".into(),
            datastore: "ds".into(),
        };
        assert_eq!(parts.tcp_connect_address(), "pbs.example.com:8007");
    }
}
