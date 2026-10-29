//! Client for rclone's remote-control API, and supervision of the `rclone rcd` process.

use std::io;
use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};

const READY_TIMEOUT: Duration = Duration::from_secs(15);
const CALL_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not start rclone: {0}")]
    Spawn(#[source] io::Error),
    #[error("rclone rcd exited early: {0}")]
    Exited(String),
    #[error("rclone rcd was not ready after {0:?}")]
    NotReady(Duration),
    #[error("rclone {method} failed: {message}")]
    Rc { method: String, message: String },
    #[error("could not reach rclone: {0}")]
    Transport(String),
    #[error("unexpected response from rclone: {0}")]
    Decode(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// One entry from `operations/list`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct ListItem {
    pub path: String,
    pub name: String,
    /// Size in bytes, or -1 for folders and Google Docs.
    pub size: i64,
    #[serde(default)]
    pub mime_type: String,
    pub mod_time: String,
    pub is_dir: bool,
    /// The Drive file ID, when the backend has one.
    #[serde(rename = "ID", default)]
    pub id: Option<String>,
}

#[derive(Deserialize)]
struct ListResponse {
    list: Vec<ListItem>,
}

#[derive(Deserialize)]
struct RcErrorBody {
    error: String,
}

/// Talks to a running `rclone rcd` over HTTP with basic auth.
#[derive(Clone)]
pub struct RcClient {
    agent: ureq::Agent,
    base_url: String,
    auth: String,
}

impl RcClient {
    pub fn new(base_url: impl Into<String>, user: &str, pass: &str) -> Self {
        let token = base64::engine::general_purpose::STANDARD.encode(format!("{user}:{pass}"));
        Self {
            agent: ureq::AgentBuilder::new().timeout(CALL_TIMEOUT).build(),
            base_url: base_url.into(),
            auth: format!("Basic {token}"),
        }
    }

    /// Calls an rc method such as `operations/list` and returns its JSON reply.
    pub fn call(&self, method: &str, params: Value) -> Result<Value> {
        let url = format!("{}/{method}", self.base_url);
        let reply = self
            .agent
            .post(&url)
            .set("Authorization", &self.auth)
            .send_json(params);
        match reply {
            Ok(resp) => resp
                .into_json()
                .map_err(|e| Error::Decode(e.to_string())),
            Err(ureq::Error::Status(code, resp)) => {
                let message = resp
                    .into_json::<RcErrorBody>()
                    .map(|b| b.error)
                    .unwrap_or_else(|_| format!("HTTP {code}"));
                Err(Error::Rc {
                    method: method.to_string(),
                    message,
                })
            }
            Err(e) => Err(Error::Transport(e.to_string())),
        }
    }

    /// The rclone version, e.g. `v1.68.1`.
    pub fn version(&self) -> Result<String> {
        let reply = self.call("core/version", json!({}))?;
        reply["version"]
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| Error::Decode("core/version reply has no version".into()))
    }

    /// Lists the entries directly inside `fs`, an rclone path such as `gdrive:Documents`.
    pub fn list(&self, fs: &str) -> Result<Vec<ListItem>> {
        let reply = self.call("operations/list", json!({ "fs": fs, "remote": "" }))?;
        parse_list(reply)
    }
}

fn parse_list(reply: Value) -> Result<Vec<ListItem>> {
    serde_json::from_value::<ListResponse>(reply)
        .map(|r| r.list)
        .map_err(|e| Error::Decode(e.to_string()))
}

/// An `rclone rcd` child process on localhost with random credentials. Killed on drop.
pub struct Rcd {
    child: Child,
    client: RcClient,
}

impl Rcd {
    /// Starts `rcd` from the given rclone binary and waits until it answers.
    pub fn start(rclone: &Path) -> Result<Self> {
        let port = free_port().map_err(Error::Spawn)?;
        let user = random_token().map_err(Error::Spawn)?;
        let pass = random_token().map_err(Error::Spawn)?;
        let child = Command::new(rclone)
            .arg("rcd")
            .arg(format!("--rc-addr=127.0.0.1:{port}"))
            // Credentials go through the environment so they don't show up in `ps`.
            .env("RCLONE_RC_USER", &user)
            .env("RCLONE_RC_PASS", &pass)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .spawn()
            .map_err(Error::Spawn)?;
        let client = RcClient::new(format!("http://127.0.0.1:{port}"), &user, &pass);
        let mut rcd = Rcd { child, client };
        rcd.wait_ready()?;
        Ok(rcd)
    }

    pub fn client(&self) -> &RcClient {
        &self.client
    }

    fn wait_ready(&mut self) -> Result<()> {
        let started = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().map_err(Error::Spawn)? {
                return Err(Error::Exited(status.to_string()));
            }
            if self.client.call("rc/noop", json!({})).is_ok() {
                return Ok(());
            }
            if started.elapsed() > READY_TIMEOUT {
                return Err(Error::NotReady(READY_TIMEOUT));
            }
            thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for Rcd {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn free_port() -> io::Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn random_token() -> io::Result<String> {
    let mut buf = [0u8; 24];
    getrandom::getrandom(&mut buf).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(buf))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_drive_listing() {
        let reply = json!({
            "list": [
                {
                    "Path": "Documents/report.pdf",
                    "Name": "report.pdf",
                    "Size": 52011,
                    "MimeType": "application/pdf",
                    "ModTime": "2026-10-20T09:15:00.000Z",
                    "IsDir": false,
                    "ID": "1AbCdEf"
                },
                {
                    "Path": "Documents/Taxes",
                    "Name": "Taxes",
                    "Size": -1,
                    "MimeType": "inode/directory",
                    "ModTime": "2026-09-01T12:00:00.000Z",
                    "IsDir": true
                }
            ]
        });
        let items = parse_list(reply).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].name, "report.pdf");
        assert_eq!(items[0].id.as_deref(), Some("1AbCdEf"));
        assert!(items[1].is_dir);
        assert_eq!(items[1].id, None);
    }

    #[test]
    fn rejects_malformed_listing() {
        assert!(matches!(
            parse_list(json!({ "items": [] })),
            Err(Error::Decode(_))
        ));
    }

    #[test]
    fn tokens_are_random() {
        assert_ne!(random_token().unwrap(), random_token().unwrap());
    }
}
