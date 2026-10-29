//! Google Drive credentials and sign-in through `rclone authorize`.

use std::path::Path;
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{Error, Result};

/// Everything rclone needs to talk to Drive with the user's own OAuth client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriveCredentials {
    pub client_id: String,
    pub client_secret: String,
    /// The token JSON printed by `rclone authorize`.
    pub token: String,
}

impl DriveCredentials {
    /// Environment variables that define rclone remote `remote` in memory, so the token
    /// never has to be written to an rclone config file.
    pub fn env(&self, remote: &str) -> Vec<(String, String)> {
        let prefix = format!("RCLONE_CONFIG_{}", remote.to_uppercase());
        vec![
            (format!("{prefix}_TYPE"), "drive".to_string()),
            (format!("{prefix}_SCOPE"), "drive".to_string()),
            (format!("{prefix}_CLIENT_ID"), self.client_id.clone()),
            (
                format!("{prefix}_CLIENT_SECRET"),
                self.client_secret.clone(),
            ),
            (format!("{prefix}_TOKEN"), self.token.clone()),
            // Google Docs are handled separately (link files, Phase 2), never synced as copies.
            ("RCLONE_DRIVE_SKIP_GDOCS".to_string(), "true".to_string()),
        ]
    }
}

/// Reads the client ID and secret from the JSON file Google Cloud Console downloads
/// for a Desktop OAuth client.
pub fn parse_client_secret_file(json: &str) -> Option<(String, String)> {
    let value: Value = serde_json::from_str(json).ok()?;
    let client = value.get("installed").or_else(|| value.get("web"))?;
    let id = client.get("client_id")?.as_str()?;
    let secret = client.get("client_secret")?.as_str()?;
    Some((id.to_string(), secret.to_string()))
}

/// Runs the browser sign-in and returns the token JSON.
pub fn authorize(rclone: &Path, client_id: &str, client_secret: &str) -> Result<String> {
    let output = Command::new(rclone)
        .args(["authorize", "drive", client_id, client_secret])
        .stdin(Stdio::null())
        // rclone prints the sign-in link on stderr; let the user see it.
        .stderr(Stdio::inherit())
        .output()
        .map_err(Error::Spawn)?;
    if !output.status.success() {
        return Err(Error::Failed {
            command: "authorize".into(),
            status: output.status.to_string(),
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_authorize_output(&stdout)
        .ok_or_else(|| Error::Decode("no token in rclone authorize output".into()))
}

fn parse_authorize_output(stdout: &str) -> Option<String> {
    let start = stdout.find("--->")? + "--->".len();
    let end = start + stdout[start..].find("<---")?;
    let token = stdout[start..end].trim();
    serde_json::from_str::<Value>(token).ok()?;
    Some(token.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_desktop_client_file() {
        let json = r#"{"installed":{"client_id":"123.apps.googleusercontent.com",
            "client_secret":"shh","redirect_uris":["http://localhost"]}}"#;
        assert_eq!(
            parse_client_secret_file(json),
            Some(("123.apps.googleusercontent.com".into(), "shh".into()))
        );
    }

    #[test]
    fn rejects_unrelated_json() {
        assert_eq!(
            parse_client_secret_file(r#"{"type":"service_account"}"#),
            None
        );
        assert_eq!(parse_client_secret_file("not json"), None);
    }

    #[test]
    fn extracts_token_from_authorize_output() {
        let out = "Paste the following into your remote machine --->\n\
                   {\"access_token\":\"ya29\",\"token_type\":\"Bearer\"}\n<---End paste\n";
        assert_eq!(
            parse_authorize_output(out).as_deref(),
            Some(r#"{"access_token":"ya29","token_type":"Bearer"}"#)
        );
    }

    #[test]
    fn rejects_authorize_output_without_token() {
        assert_eq!(parse_authorize_output("Failed to get token"), None);
        assert_eq!(parse_authorize_output("---> not json <---"), None);
    }

    #[test]
    fn env_defines_remote_in_memory() {
        let creds = DriveCredentials {
            client_id: "id".into(),
            client_secret: "secret".into(),
            token: "{}".into(),
        };
        let env = creds.env("drivebridge");
        assert!(env.contains(&("RCLONE_CONFIG_DRIVEBRIDGE_TYPE".into(), "drive".into())));
        assert!(env.contains(&("RCLONE_CONFIG_DRIVEBRIDGE_TOKEN".into(), "{}".into())));
    }
}
