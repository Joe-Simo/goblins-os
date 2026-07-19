//! Read operator-provisioned OpenAI configuration from systemd credentials.
//!
//! Secrets must not live in the core process environment: every subprocess
//! inherits that environment unless it is perfectly isolated. systemd instead
//! copies `LoadCredential=` and decrypted `LoadCredentialEncrypted=` payloads
//! into the directory named by `CREDENTIALS_DIRECTORY`. Only the auth and relay
//! paths call this module, and values are read on demand without exporting them.

use std::{collections::HashMap, env, fs, path::Path};

const OPENAI_CREDENTIAL_FILE: &str = "openai-secrets.env";
const MAX_CREDENTIAL_BYTES: u64 = 64 * 1024;

/// Read one value from the OpenAI service credential. Invalid, ambiguous, or
/// oversized credential files fail closed and make the provider unavailable.
pub(crate) fn openai_credential(name: &str) -> Option<String> {
    let directory = env::var_os("CREDENTIALS_DIRECTORY")?;
    openai_credential_from_dir(Path::new(&directory), name)
}

pub(crate) fn openai_credential_with_compat(primary: &str, legacy: &str) -> Option<String> {
    openai_credential(primary).or_else(|| openai_credential(legacy))
}

fn openai_credential_from_dir(directory: &Path, name: &str) -> Option<String> {
    if !valid_name(name) {
        return None;
    }
    let path = directory.join(OPENAI_CREDENTIAL_FILE);
    let metadata = fs::metadata(&path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_CREDENTIAL_BYTES {
        return None;
    }
    let contents = fs::read_to_string(path).ok()?;
    parse_credential(&contents).ok()?.remove(name)
}

fn parse_credential(contents: &str) -> Result<HashMap<String, String>, ()> {
    if contents.len() as u64 > MAX_CREDENTIAL_BYTES {
        return Err(());
    }

    let mut values = HashMap::new();
    for line in contents.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, raw_value) = line.split_once('=').ok_or(())?;
        let name = name.trim();
        if !valid_name(name) || values.contains_key(name) {
            return Err(());
        }
        let value = parse_value(raw_value.trim())?;
        values.insert(name.to_string(), value);
    }
    Ok(values)
}

fn valid_name(name: &str) -> bool {
    let mut characters = name.chars();
    characters
        .next()
        .is_some_and(|character| character.is_ascii_uppercase() || character == '_')
        && characters.all(|character| {
            character.is_ascii_uppercase() || character.is_ascii_digit() || character == '_'
        })
}

/// Credential values are literal UTF-8. Matching outer single or double quotes
/// are accepted for compatibility with existing EnvironmentFile provisioning,
/// but shell expansion and escape processing are deliberately not supported.
fn parse_value(raw: &str) -> Result<String, ()> {
    let first = raw.chars().next();
    if matches!(first, Some('\'') | Some('"')) {
        let quote = first.ok_or(())?;
        if raw.len() < 2 || !raw.ends_with(quote) {
            return Err(());
        }
        let inner = &raw[quote.len_utf8()..raw.len() - quote.len_utf8()];
        if inner.contains(quote) {
            return Err(());
        }
        return valid_value(inner).then(|| inner.to_string()).ok_or(());
    }
    valid_value(raw).then(|| raw.to_string()).ok_or(())
}

fn valid_value(value: &str) -> bool {
    !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::{openai_credential_from_dir, parse_credential, MAX_CREDENTIAL_BYTES};
    use std::{fs, path::PathBuf};

    fn test_directory(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "goblins-os-credential-{label}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ))
    }

    #[test]
    fn strict_credential_parser_supports_existing_assignment_shape() {
        let parsed = parse_credential(
            r#"
                # operator configuration
                OPENAI_ACCOUNT_CLIENT_ID=client-id
                OPENAI_ACCOUNT_SCOPE='openid profile email'
                OPENAI_API_KEY=<test-openai-key>
                AI_GATEWAY_API_KEY=<test-gateway-key>
            "#,
        )
        .expect("valid credential payload");
        assert_eq!(parsed["OPENAI_ACCOUNT_CLIENT_ID"], "client-id");
        assert_eq!(parsed["OPENAI_ACCOUNT_SCOPE"], "openid profile email");
        assert_eq!(parsed["OPENAI_API_KEY"], "<test-openai-key>");
        assert_eq!(parsed["AI_GATEWAY_API_KEY"], "<test-gateway-key>");
    }

    #[test]
    fn malformed_or_ambiguous_credential_payload_fails_closed() {
        for payload in [
            "not-an-assignment",
            "lowercase=value",
            "DUPLICATE=one\nDUPLICATE=two",
            "BROKEN='unterminated",
            "BROKEN='nested'quote'",
            "CONTROL=value\twith-tab",
        ] {
            assert!(parse_credential(payload).is_err(), "accepted {payload:?}");
        }
        assert!(parse_credential(&"X".repeat(MAX_CREDENTIAL_BYTES as usize + 1)).is_err());
    }

    #[test]
    fn credential_reader_uses_only_the_systemd_credential_directory() {
        let directory = test_directory("read");
        fs::create_dir_all(&directory).expect("create credential directory");
        fs::write(
            directory.join("openai-secrets.env"),
            "AI_GATEWAY_API_KEY=<credential-test-value>\n",
        )
        .expect("write credential payload");

        assert_eq!(
            openai_credential_from_dir(&directory, "AI_GATEWAY_API_KEY").as_deref(),
            Some("<credential-test-value>")
        );
        assert!(openai_credential_from_dir(&directory, "../escape").is_none());

        fs::remove_dir_all(directory).expect("remove credential directory");
    }
}
