use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncWrite, AsyncWriteExt},
    process::ChildStdin,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

struct Input {
    text: String,
    eof: bool,
    size: Option<(u16, u16)>,
    reply: oneshot::Sender<Value>,
}
#[derive(Clone)]
struct Session {
    owner: String,
    sender: mpsc::Sender<Input>,
    #[cfg(windows)]
    pty: Option<Arc<crate::conpty::Control>>,
}
#[derive(Clone, Default)]
pub struct Sessions(Arc<Mutex<HashMap<String, Session>>>);
pub struct Registration {
    sessions: Sessions,
    task_id: String,
    writer: JoinHandle<()>,
}
impl Drop for Registration {
    fn drop(&mut self) {
        if let Ok(mut sessions) = self.sessions.0.lock() {
            sessions.remove(&self.task_id);
        }
        self.writer.abort();
    }
}
impl Sessions {
    pub fn attach(&self, owner: &str, task_id: &str, stdin: ChildStdin) -> Result<Registration> {
        self.attach_writer(
            owner,
            task_id,
            stdin,
            #[cfg(windows)]
            None,
        )
    }
    #[cfg(windows)]
    pub fn attach_pty(
        &self,
        owner: &str,
        task_id: &str,
        stdin: tokio::fs::File,
        control: Arc<crate::conpty::Control>,
    ) -> Result<Registration> {
        self.attach_writer(owner, task_id, stdin, Some(control))
    }
    fn attach_writer<W: AsyncWrite + Unpin + Send + 'static>(
        &self,
        owner: &str,
        task_id: &str,
        mut stdin: W,
        #[cfg(windows)] pty: Option<Arc<crate::conpty::Control>>,
    ) -> Result<Registration> {
        let (sender, mut receiver) = mpsc::channel::<Input>(8);
        let mut sessions = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Terminal session lock failed"))?;
        ensure!(
            !sessions.contains_key(task_id),
            "Terminal session already exists"
        );
        sessions.insert(
            task_id.to_owned(),
            Session {
                owner: owner.to_owned(),
                sender,
                #[cfg(windows)]
                pty: pty.clone(),
            },
        );
        let writer = tokio::spawn(async move {
            while let Some(input) = receiver.recv().await {
                let result = tokio::time::timeout(Duration::from_secs(3), async {
                    #[cfg(windows)]
                    if let Some(size) = input.size {
                        pty.as_ref()
                            .ok_or_else(|| std::io::Error::other("Resize requires ConPTY"))?
                            .resize(size)
                            .map_err(std::io::Error::other)?;
                    }
                    stdin.write_all(input.text.as_bytes()).await?;
                    stdin.flush().await?;
                    if input.eof {
                        stdin.shutdown().await?;
                    }
                    Ok::<_, std::io::Error>(())
                })
                .await;
                let ok = matches!(result, Ok(Ok(())));
                let response = if ok {
                    json!({"input_sent":!input.text.is_empty(),"bytes_sent":input.text.len(),"stdin_closed":input.eof,"resized":input.size.is_some(),"verification_state":"sent","verification_note":"Input was delivered; inspect output or terminal_screen for the application effect"})
                } else {
                    json!({"uncertain":true,"error":"Stdin write did not complete; bytes may have been partially sent. Inspect output before retrying."})
                };
                let _ = input.reply.send(response);
                if !ok || input.eof {
                    break;
                }
            }
        });
        Ok(Registration {
            sessions: self.clone(),
            task_id: task_id.to_owned(),
            writer,
        })
    }
    pub async fn send(&self, owner: &str, args: &Value) -> Result<Value> {
        let id = args["target_task_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing target task"))?;
        let mut text = args["text"].as_str().unwrap_or("").to_owned();
        let eof = args["eof"].as_bool().unwrap_or(false);
        let session = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Terminal session lock failed"))?
            .get(id)
            .filter(|session| session.owner == owner)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Owned interactive terminal is not ready or has ended")
            })?;
        let size = match (args["cols"].as_u64(), args["rows"].as_u64()) {
            (None, None) => None,
            (Some(cols), Some(rows)) if (20..=240).contains(&cols) && (5..=100).contains(&rows) => {
                Some((cols as u16, rows as u16))
            }
            _ => anyhow::bail!("Resize requires cols 20..240 and rows 5..100 together"),
        };
        let keys = args["keys"].as_array();
        #[cfg(windows)]
        if let Some(control) = session.pty.as_ref() {
            ensure!(
                !eof,
                "ConPTY has no pipe EOF; use CTRL_Z/CTRL_D as the program requires, or task_cancel"
            );
            if let Some(keys) = keys {
                ensure!(keys.len() <= 64, "Too many keys");
                for key in keys {
                    text.push_str(&key_bytes(
                        key.as_str().ok_or_else(|| anyhow::anyhow!("Invalid key"))?,
                        control.application_cursor(),
                    )?);
                }
            }
        } else {
            ensure!(
                size.is_none() && keys.is_none_or(|keys| keys.is_empty()),
                "Keyboard keys and resize require ConPTY"
            );
        }
        #[cfg(not(windows))]
        ensure!(
            size.is_none() && keys.is_none_or(|keys| keys.is_empty()),
            "ConPTY requires Windows"
        );
        ensure!(
            text.len() <= 16384 && (!text.is_empty() || eof || size.is_some()),
            "Input requires bounded text, keys, resize or eof=true"
        );
        let (reply, received) = oneshot::channel();
        session
            .sender
            .try_send(Input {
                text,
                eof,
                size,
                reply,
            })
            .map_err(|_| {
                anyhow::anyhow!(
                    "Terminal input is closed or its queue is full; input was not queued"
                )
            })?;
        match tokio::time::timeout(Duration::from_secs(5), received).await {
            Ok(Ok(mut result)) => {
                result["target_task_id"] = json!(id);
                Ok(result)
            }
            _ => Ok(
                json!({"target_task_id":id,"uncertain":true,"error":"Stdin acknowledgement unavailable; inspect output before retrying"}),
            ),
        }
    }
    pub fn screen(&self, owner: &str, args: &Value) -> Result<Value> {
        let id = args["target_task_id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing target task"))?;
        let sessions = self
            .0
            .lock()
            .map_err(|_| anyhow::anyhow!("Terminal session lock failed"))?;
        let session = sessions
            .get(id)
            .filter(|session| session.owner == owner)
            .ok_or_else(|| {
                anyhow::anyhow!("Owned interactive terminal is not ready or has ended")
            })?;
        #[cfg(windows)]
        return session
            .pty
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Screen read requires ConPTY"))?
            .screen();
        #[cfg(not(windows))]
        {
            let _ = session;
            anyhow::bail!("ConPTY requires Windows")
        }
    }
}
#[cfg(windows)]
fn key_bytes(key: &str, application: bool) -> Result<String> {
    let prefix = if application { "\x1bO" } else { "\x1b[" };
    let bytes = match key {
        "ENTER" => "\r".into(),
        "TAB" => "\t".into(),
        "ESC" => "\x1b".into(),
        "BACKSPACE" => "\x7f".into(),
        "SHIFT_TAB" => "\x1b[Z".into(),
        "UP" => format!("{prefix}A"),
        "DOWN" => format!("{prefix}B"),
        "RIGHT" => format!("{prefix}C"),
        "LEFT" => format!("{prefix}D"),
        "HOME" => format!("{prefix}H"),
        "END" => format!("{prefix}F"),
        "INSERT" => "\x1b[2~".into(),
        "DELETE" => "\x1b[3~".into(),
        "PAGEUP" => "\x1b[5~".into(),
        "PAGEDOWN" => "\x1b[6~".into(),
        "F1" => "\x1bOP".into(),
        "F2" => "\x1bOQ".into(),
        "F3" => "\x1bOR".into(),
        "F4" => "\x1bOS".into(),
        "F5" => "\x1b[15~".into(),
        "F6" => "\x1b[17~".into(),
        "F7" => "\x1b[18~".into(),
        "F8" => "\x1b[19~".into(),
        "F9" => "\x1b[20~".into(),
        "F10" => "\x1b[21~".into(),
        "F11" => "\x1b[23~".into(),
        "F12" => "\x1b[24~".into(),
        _ if key.starts_with("CTRL_")
            && key.len() == 6
            && key.as_bytes()[5].is_ascii_uppercase() =>
        {
            ((key.as_bytes()[5] - b'A' + 1) as char).to_string()
        }
        _ if key.starts_with("ALT_")
            && key.len() == 5
            && key.as_bytes()[4].is_ascii_alphanumeric() =>
        {
            format!("\x1b{}", key.as_bytes()[4] as char)
        }
        _ => anyhow::bail!("Unsupported terminal key: {key}"),
    };
    Ok(bytes)
}
