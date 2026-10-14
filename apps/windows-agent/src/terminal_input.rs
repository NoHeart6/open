use anyhow::{Result, ensure};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::AsyncWriteExt,
    process::ChildStdin,
    sync::{mpsc, oneshot},
    task::JoinHandle,
};

struct Input {
    text: String,
    eof: bool,
    reply: oneshot::Sender<Value>,
}
#[derive(Clone)]
struct Session {
    owner: String,
    sender: mpsc::Sender<Input>,
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
    pub fn attach(
        &self,
        owner: &str,
        task_id: &str,
        mut stdin: ChildStdin,
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
            },
        );
        let writer = tokio::spawn(async move {
            while let Some(input) = receiver.recv().await {
                let result = tokio::time::timeout(Duration::from_secs(3), async {
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
                    json!({"input_sent":true,"bytes_sent":input.text.len(),"stdin_closed":input.eof,"verification_state":"sent","verification_note":"Bytes were written to the stdin pipe; inspect terminal output for the application effect"})
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
        let text = args["text"].as_str().unwrap_or("");
        let eof = args["eof"].as_bool().unwrap_or(false);
        ensure!(
            text.len() <= 16384 && (!text.is_empty() || eof),
            "Stdin requires bounded text or eof=true"
        );
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
        let (reply, received) = oneshot::channel();
        session
            .sender
            .try_send(Input {
                text: text.to_owned(),
                eof,
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
}
