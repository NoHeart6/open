#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod asset_bridge;
mod automation;
mod batch;
mod clipboard;
#[cfg(windows)]
mod conpty;
mod desktop;
mod execution;
mod file_export;
mod files;
mod history;
mod image_place;
mod installation;
mod media_transfer;
mod motiongraphics;
mod policy;
mod recovery;
mod storage;
mod system;
mod terminal_input;
mod ui;
mod updater;
mod visual_agent;
mod wordcraft;
mod wordcraft_compose;
mod workflow;

use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use clap::Parser;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use futures_util::{SinkExt, StreamExt};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, mpsc};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};

#[derive(Parser)]
#[command(version, about = "OpenCommander per-user Windows agent")]
struct Cli {
    #[arg(long)]
    init: bool,
    #[arg(long)]
    status: bool,
    #[arg(long)]
    background: bool,
    #[arg(long)]
    ui: bool,
    #[arg(long)]
    install: bool,
    #[arg(long)]
    upgrade: bool,
    #[arg(long)]
    no_launch: bool,
    #[arg(long)]
    uninstall: bool,
    #[arg(long)]
    forget: bool,
    #[arg(long)]
    gateway: Option<String>,
    #[arg(long)]
    root: Vec<PathBuf>,
    #[arg(long)]
    application: Vec<PathBuf>,
    #[arg(long)]
    allow_modify: bool,
    #[arg(long)]
    allow_screenshots: bool,
    #[arg(long)]
    dev: bool,
    #[arg(long)]
    state: Option<PathBuf>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    gateway: String,
    policy: policy::Policy,
    dev: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Credentials {
    device_id: String,
    owner: String,
    seed: [u8; 32],
    gateway_public_key: String,
}
#[derive(Clone, Serialize, Deserialize)]
struct Task {
    task_id: String,
    owner: String,
    device_id: String,
    action: String,
    args: Value,
    status: String,
    category: String,
    created_at: u64,
    deadline: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    task_id: String,
    status: String,
    result: Value,
    error: String,
    fingerprint: String,
    #[serde(default)]
    acknowledged_at: u64,
}
type Cancellations = Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>;
const MAX_CONCURRENT_TERMINALS: usize = 4;
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn journal_path(state: &Path, id: &str) -> Result<PathBuf> {
    ensure!(
        (8..=128).contains(&id.len())
            && id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-'),
        "Invalid task identifier"
    );
    Ok(state.join("journal").join(format!("{id}.dpapi")))
}
fn save_record(state: &Path, record: &Record) -> Result<()> {
    storage::save_bounded(
        &journal_path(state, &record.task_id)?,
        &serde_json::to_vec(record)?,
        4096,
        64 * 1024 * 1024,
    )
}
fn update(record: &Record) -> Value {
    json!({"type":"result","task_id":record.task_id,"status":record.status,"result":record.result,"error":record.error})
}
fn log_event(state: &Path, value: Value) -> Result<()> {
    use std::io::Write;
    let path = state.join("events.log");
    if path.metadata().is_ok_and(|m| m.len() >= 2 * 1024 * 1024) {
        let oldest = state.join("events.2.log");
        if oldest.exists() {
            std::fs::remove_file(&oldest)?;
        }
        let previous = state.join("events.1.log");
        if previous.exists() {
            std::fs::rename(previous, oldest)?;
        }
        std::fs::rename(&path, state.join("events.1.log"))?;
    }
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", json!({"at":now(),"event":value}))?;
    println!("{value}");
    Ok(())
}
fn connection_state(state: &Path, connected: bool) -> Result<()> {
    storage::save(
        &state.join("connection.dpapi"),
        &serde_json::to_vec(
            &json!({"pid":std::process::id(),"connected":connected,"updated_at":now()}),
        )?,
    )
}
fn fingerprint(t: &Task) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(format!(
        "{}:{}:{}",
        t.action, t.args, t.deadline
    )))
}
fn validate_gateway(raw: &str, dev: bool) -> Result<url::Url> {
    let url = url::Url::parse(raw)?;
    ensure!(
        url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
            && url.path() == "/",
        "Gateway must be an origin URL"
    );
    if dev {
        ensure!(
            url.scheme() == "http" && url.host_str() == Some("127.0.0.1"),
            "Development gateway must use loopback HTTP"
        );
    } else {
        ensure!(url.scheme() == "https", "Production gateway requires HTTPS");
    }
    Ok(url)
}
async fn enroll(state: &Path, cfg: &Config, control: &ui::Control) -> Result<Credentials> {
    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);
    let key = SigningKey::from_bytes(&seed);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let base = validate_gateway(&cfg.gateway, cfg.dev)?;
    let response:Value=client.post(base.join("v1/enroll")?).json(&json!({"public_key":STANDARD.encode(key.verifying_key().as_bytes()),"name":std::env::var("COMPUTERNAME").unwrap_or_else(|_|"Windows PC".into())})).send().await?.error_for_status()?.json().await?;
    let id = response["enrollment_id"]
        .as_str()
        .context("Missing enrollment id")?;
    let secret = response["poll_secret"]
        .as_str()
        .context("Missing poll secret")?;
    storage::save(
        &state.join("pending.dpapi"),
        &serde_json::to_vec(&json!({"seed":seed,"enrollment_id":id,"secret":secret}))?,
    )?;
    println!(
        "{}",
        json!({"event":"pairing_required","code":response["pairing_code"],"expires_at":response["expires_at"]})
    );
    storage::save(
        &state.join("pairing-view.dpapi"),
        &serde_json::to_vec(
            &json!({"code":response["pairing_code"],"expires":response["expires_at"]}),
        )?,
    )?;
    while now()
        < response["expires_at"]
            .as_u64()
            .context("Missing pairing deadline")?
    {
        consume_local_control(state, control)?;
        ensure!(
            !control.shutdown.load(Ordering::SeqCst),
            "Pairing cancelled by local user"
        );
        tokio::time::sleep(Duration::from_secs(2)).await;
        let response = client
            .post(base.join(&format!("v1/enroll/{id}/poll"))?)
            .json(&json!({"secret":secret}))
            .send()
            .await?;
        if response.status() == reqwest::StatusCode::ACCEPTED {
            continue;
        }
        let paired: Value = response.error_for_status()?.json().await?;
        let creds = Credentials {
            device_id: paired["device_id"]
                .as_str()
                .context("Missing paired device")?
                .into(),
            owner: paired["owner"].as_str().context("Missing owner")?.into(),
            seed,
            gateway_public_key: paired["gateway_public_key"]
                .as_str()
                .context("Missing gateway signing key")?
                .into(),
        };
        storage::save(
            &state.join("credentials.dpapi"),
            &serde_json::to_vec(&creds)?,
        )?;
        std::fs::remove_file(state.join("pending.dpapi"))?;
        std::fs::remove_file(state.join("pairing-view.dpapi"))?;
        println!("{}", json!({"event":"paired","device_id":creds.device_id}));
        return Ok(creds);
    }
    bail!("Pairing code expired; run --init again")
}
fn verify_task(message: &Value, creds: &Credentials) -> Result<Task> {
    let payload = STANDARD.decode(
        message["payload"]
            .as_str()
            .context("Missing signed payload")?,
    )?;
    ensure!(payload.len() <= 256 * 1024, "Task too large");
    let key: [u8; 32] = STANDARD
        .decode(&creds.gateway_public_key)?
        .try_into()
        .map_err(|_| anyhow::anyhow!("Invalid gateway key"))?;
    let signature = Signature::from_slice(
        &STANDARD.decode(message["signature"].as_str().context("Missing signature")?)?,
    )?;
    VerifyingKey::from_bytes(&key)?.verify(&payload, &signature)?;
    let task: Task = serde_json::from_slice(&payload)?;
    ensure!(
        task.owner == creds.owner && task.device_id == creds.device_id,
        "Task ownership mismatch"
    );
    ensure!(
        task.deadline > now() && task.deadline <= now() + 600,
        "Task expired or invalid deadline"
    );
    ensure!(task.args.is_object(), "Arguments must be an object");
    ensure!(
        policy::category(&task.action, &task.args)? == task.category,
        "Server action category mismatch"
    );
    Ok(task)
}
#[derive(Clone)]
struct TaskScheduler {
    desktop: Arc<Semaphore>,
    terminals: Arc<Semaphore>,
    sessions: terminal_input::Sessions,
}
impl Default for TaskScheduler {
    fn default() -> Self {
        Self {
            desktop: Arc::new(Semaphore::new(1)),
            terminals: Arc::new(Semaphore::new(MAX_CONCURRENT_TERMINALS)),
            sessions: terminal_input::Sessions::default(),
        }
    }
}
struct TaskRuntime {
    state: PathBuf,
    sender: mpsc::Sender<Value>,
    cancels: Cancellations,
    scheduler: TaskScheduler,
    control: Arc<ui::Control>,
    streaming: bool,
}
async fn run_task(t: Task, flag: Arc<AtomicBool>, runtime: TaskRuntime) -> Result<()> {
    let TaskRuntime {
        state,
        sender,
        cancels,
        scheduler,
        control,
        streaming,
    } = runtime;
    let path = journal_path(&state, &t.task_id)?;
    let hash = fingerprint(&t);
    if path.exists() {
        let record: Record = serde_json::from_slice(&storage::load(&path)?)?;
        ensure!(
            record.fingerprint == hash,
            "Task ID reused with different action"
        );
        sender.send(update(&record)).await?;
        cancels
            .lock()
            .map_err(|_| anyhow::anyhow!("Cancellation lock failed"))?
            .remove(&t.task_id);
        return Ok(());
    }
    let mut record = Record {
        task_id: t.task_id.clone(),
        status: "running".into(),
        result: Value::Null,
        error: String::new(),
        fingerprint: hash,
        acknowledged_at: 0,
    };
    // Commit intent before any local side effects. Recovery never re-executes an incomplete record.
    save_record(&state, &record)?;
    let result: Result<Value> = async {
        let remaining = t.deadline.saturating_sub(now());
        ensure!(remaining > 0, "Deadline expired");
        let lane = if t.action == "terminal_execute" { &scheduler.terminals } else { &scheduler.desktop };
        let _permit = if matches!(t.action.as_str(), "terminal_input" | "terminal_screen") { None } else {
            Some(tokio::time::timeout(Duration::from_secs(remaining), lane.acquire()).await.context("Execution slot unavailable before task deadline")??)
        };
        ensure!(
            !flag.load(Ordering::SeqCst) && now() < t.deadline && control.mode.load(Ordering::SeqCst) == 0,
            "Task cancelled or expired before execution"
        );
        let mut current_cfg: Config = serde_json::from_slice(&std::fs::read(state.join("config.json"))?)?;
        current_cfg.policy.validate()?;
        current_cfg.policy.protect_internal(&state)?;
        let category = current_cfg.policy.authorize(&t.action, &t.args)?;
        if current_cfg.policy.needs_consent(category) {
            record.status = "approval_pending".into();
            save_record(&state, &record)?;
            sender.send(update(&record)).await?;
            let owner = t.owner.clone();
            let id = t.task_id.clone();
            let action = t.action.clone();
            let args = t.args.clone();
            let mut approval =
                tokio::task::spawn_blocking(move || desktop::consent(&owner, &id, &action, &args));
            let granted=loop {
                tokio::select! {
                    result=&mut approval=>break result??,
                    _=tokio::time::sleep(Duration::from_millis(200))=>{
                        if flag.load(Ordering::SeqCst)||now()>=t.deadline {let _=desktop::close_consent(&t.task_id);bail!("Consent cancelled or expired");}
                    }
                }
            };
            ensure!(granted, "Local user denied this operation");
        } else if category == "SENSITIVE" {
            log_event(&state, json!({"event":"local_owner_preapproval_used","task_id":t.task_id,"action":t.action}))?;
        }
        ensure!(
            !flag.load(Ordering::SeqCst) && now() < t.deadline && control.mode.load(Ordering::SeqCst) == 0,
            "Task cancelled or expired before execution"
        );
        record.status = "running".into();
        save_record(&state, &record)?;
        sender.send(update(&record)).await?;
        let mut output = execution::execute(
            &current_cfg.policy,
            &t.action,
            t.args.clone(),
            execution::Context {
                cancel:flag.clone(), seconds:t.deadline.saturating_sub(now()),
                output:streaming.then(|| execution::Output { state:state.clone(), task_id:t.task_id.clone(), sender:sender.clone(), sequence:Arc::new(std::sync::atomic::AtomicU64::new(0)) }),
                state:&state,owner:&t.owner,task_id:&t.task_id,sessions:&scheduler.sessions,
            },
        )
        .await?;
        if t.action == "system_info" {
            output["agent_version"] = json!(env!("CARGO_PKG_VERSION"));
            output["workspace"] = json!({"user_profile":std::env::var("USERPROFILE").ok(),"approved_roots":current_cfg.policy.roots,"permission_modify":current_cfg.policy.allow_modify,"permission_screenshots":current_cfg.policy.allow_screenshots,"ask_before_sensitive":current_cfg.policy.ask_before_sensitive,"allow_all_applications":current_cfg.policy.allow_all_applications,"approved_applications":current_cfg.policy.applications,"internal_storage_protected":true,"max_concurrent_terminals":MAX_CONCURRENT_TERMINALS,"terminal_stdin":true});
        }
        Ok(output)
    }
    .await;
    match result {
        Ok(value) => {
            if value["uncertain"] == true {
                record.status = "uncertain".into();
                record.error = value["error"]
                    .as_str()
                    .unwrap_or("Operation outcome requires reconciliation")
                    .into();
            } else if value["completed"] == false {
                record.status = "failed".into();
                record.error = value["error"].as_str().unwrap_or("Operation failed").into();
            } else {
                record.status = "succeeded".into();
            }
            record.result = value;
        }
        Err(error) => {
            record.status = if flag.load(Ordering::SeqCst) {
                "cancelled"
            } else {
                "failed"
            }
            .into();
            record.error = format!("{error:#}").chars().take(1024).collect();
        }
    }
    history::touch(&state, now())?;
    save_record(&state, &record)?;
    cancels
        .lock()
        .map_err(|_| anyhow::anyhow!("Cancellation lock failed"))?
        .remove(&t.task_id);
    // A disconnected writer may drop this update. The next session reconciles the persisted record.
    let _ = sender.send(update(&record)).await;
    log_event(
        &state,
        json!({"event":"task_completed","task_id":t.task_id,"status":record.status}),
    )?;
    Ok(())
}
async fn connect(
    state: &Path,
    cfg: &Config,
    creds: &Credentials,
    cancels: Cancellations,
    scheduler: TaskScheduler,
    control: Arc<ui::Control>,
) -> Result<()> {
    let mut url = validate_gateway(&cfg.gateway, cfg.dev)?;
    url.set_scheme(if cfg.dev { "ws" } else { "wss" })
        .map_err(|_| anyhow::anyhow!("Invalid transport scheme"))?;
    url.set_path("/v1/agent");
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let nonce = STANDARD.encode(nonce);
    let timestamp = now();
    let signature = SigningKey::from_bytes(&creds.seed)
        .sign(format!("{}\n{}\n{}", creds.device_id, timestamp, nonce).as_bytes());
    let proof = json!({"device_id":creds.device_id,"timestamp":timestamp,"nonce":nonce,"signature":STANDARD.encode(signature.to_bytes()),"journal_ack":true});
    let mut request = url.as_str().into_client_request()?;
    request.headers_mut().insert(
        "X-Device-Proof",
        STANDARD.encode(serde_json::to_vec(&proof)?).parse()?,
    );
    let (stream, _) =
        tokio::time::timeout(Duration::from_secs(15), connect_async(request)).await??;
    let (mut write, mut read) = stream.split();
    let (sender, mut receiver) = mpsc::channel::<Value>(64);
    let writer = tokio::spawn(async move {
        while let Some(v) = receiver.recv().await {
            write.send(Message::Text(v.to_string().into())).await?;
        }
        Ok::<_, anyhow::Error>(())
    });
    connection_state(state, true)?;
    log_event(
        state,
        json!({"event":"connected","device_id":creds.device_id}),
    )?;
    for entry in std::fs::read_dir(state.join("journal"))? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|s| s == "dpapi") {
            let record: Record = serde_json::from_slice(&storage::load(&entry.path())?)?;
            if record.acknowledged_at == 0
                && !matches!(record.status.as_str(), "running" | "approval_pending")
            {
                sender.send(update(&record)).await?;
            }
        }
    }
    let mut heartbeat = tokio::time::interval(Duration::from_secs(10));
    let mut last = tokio::time::Instant::now();
    let generation = control.reconnect.load(Ordering::SeqCst);
    let mut streaming = false;
    let result = loop {
        tokio::select! {
            _=tokio::signal::ctrl_c()=>break Ok(()),
            _=tokio::time::sleep(Duration::from_millis(200))=>{
                consume_local_control(state, &control)?;
                if control.shutdown.load(Ordering::SeqCst) { break Ok(()); }
                if control.mode.load(Ordering::SeqCst)==2 || control.reconnect.load(Ordering::SeqCst)!=generation { break Err(anyhow::anyhow!("Local connection control changed")); }
            },
            _=heartbeat.tick()=>{
                if last.elapsed()>Duration::from_secs(40){break Err(anyhow::anyhow!("Gateway heartbeat stale"))}
                if sender.send(json!({"type":"heartbeat"})).await.is_err(){break Err(anyhow::anyhow!("Writer disconnected"))}
                connection_state(state,true)?;
                if streaming { reconcile_output(state,&sender)?; }
                for entry in std::fs::read_dir(state.join("journal"))? {
                    let entry=entry?;
                    if entry.path().extension().is_some_and(|s|s=="dpapi") {
                        let record:Record=serde_json::from_slice(&storage::load(&entry.path())?)?;
                        if record.acknowledged_at == 0 && !matches!(record.status.as_str(),"running"|"approval_pending") {sender.send(update(&record)).await?;}
                    }
                }
            },
            message=read.next()=>{
                match message {
                    Some(Ok(Message::Text(text)))=>{
                        last=tokio::time::Instant::now();if text.len()>512*1024{break Err(anyhow::anyhow!("Gateway frame too large"))}
                        let v:Value=match serde_json::from_str(&text){Ok(v)=>v,Err(e)=>break Err(e.into())};
                        match v["type"].as_str(){
                            Some("hello")=>{
                                if v["protocol_version"]!=1{break Err(anyhow::anyhow!("Incompatible gateway protocol"))}
                                if let Some(at)=v["mcp_activity_at"].as_u64(){history::touch(state,at)?;}
                                streaming=v["streaming_output"]==true;
                                if streaming {reconcile_output(state,&sender)?;}
                            },
                            Some("heartbeat")=>{
                                if let Some(at)=v["mcp_activity_at"].as_u64(){history::touch(state,at)?;}
                                let active=cancels.lock().map_err(|_|anyhow::anyhow!("Cancellation lock failed"))?;
                                history::prune(state,now(),!active.is_empty())?;
                            },
                            Some("ack")=>{ acknowledge(state, &v)?; },
                            Some("output_ack")=>{
                                let path=execution::output_path(state,v["task_id"].as_str().context("Missing output task")?,v["seq"].as_u64().context("Missing output sequence")?)?;
                                if path.exists(){std::fs::remove_file(path)?;}
                            },
                            Some("cancel")=>{if let Some(id)=v["task_id"].as_str() && let Some(flag)=cancels.lock().map_err(|_|anyhow::anyhow!("Cancellation lock failed"))?.get(id){flag.store(true,Ordering::SeqCst);}},
                            Some("task")=>{
                                let t=match verify_task(&v,creds){Ok(t)=>t,Err(e)=>{println!("{}",json!({"event":"request_rejected","error":e.to_string()}));continue}};
                                history::touch(state,now())?;
                                // Reserve the task in the receive loop to reject concurrent duplicate delivery.
                                let flag=Arc::new(AtomicBool::new(false));
                                {let mut map=cancels.lock().map_err(|_|anyhow::anyhow!("Cancellation lock failed"))?;
                                if map.contains_key(&t.task_id){continue}
                                if map.len()>=16{sender.try_send(json!({"type":"result","task_id":t.task_id,"status":"failed","error":"Local task capacity exceeded","result":null})).ok();continue}
                                map.insert(t.task_id.clone(),flag.clone());}
                                let (state,sender,cancels,scheduler,control)=(state.to_path_buf(),sender.clone(),cancels.clone(),scheduler.clone(),control.clone());
                                tokio::spawn(async move{let id=t.task_id.clone();let cleanup=cancels.clone();let runtime=TaskRuntime{state,sender,cancels,scheduler,control,streaming};if let Err(e)=run_task(t,flag,runtime).await{if let Ok(mut map)=cleanup.lock(){map.remove(&id);}println!("{}",json!({"event":"task_failed","task_id":id,"error":e.to_string()}));}});
                            },
                            _=>break Err(anyhow::anyhow!("Unexpected gateway message")),
                        }
                    },
                    Some(Ok(Message::Ping(_)))=>(),
                    Some(Ok(Message::Close(_)))|None=>break Err(anyhow::anyhow!("Gateway disconnected")),
                    Some(Err(e))=>break Err(e.into()),
                    _=>(),
                }
            }
        }
    };
    writer.abort();
    result
}
fn reconcile_output(state: &Path, sender: &mpsc::Sender<Value>) -> Result<()> {
    for entry in std::fs::read_dir(state.join("output"))? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|s| s == "dpapi") {
            let chunk: Value = serde_json::from_slice(&storage::load(&entry.path())?)?;
            if sender.try_send(chunk).is_err() {
                break;
            }
        }
    }
    Ok(())
}
fn consume_local_control(state: &Path, control: &ui::Control) -> Result<()> {
    let path = state.join("local-control.dpapi");
    if !path.exists() {
        return Ok(());
    }
    let v: Value = serde_json::from_slice(&storage::load(&path)?)?;
    std::fs::remove_file(path)?;
    if v["pid"].as_u64() == Some(u64::from(std::process::id()))
        && now().saturating_sub(v["at"].as_u64().unwrap_or(0)) < 30
        && v["shutdown"] == true
    {
        control.shutdown.store(true, Ordering::SeqCst);
    }
    Ok(())
}
fn acknowledge(state: &Path, v: &Value) -> Result<()> {
    let path = journal_path(state, v["task_id"].as_str().context("Missing ACK task")?)?;
    if !path.exists() {
        return Ok(());
    }
    let mut record: Record = serde_json::from_slice(&storage::load(&path)?)?;
    ensure!(
        record.status == v["status"],
        "ACK status differs from local outcome"
    );
    ensure!(
        !matches!(record.status.as_str(), "running" | "approval_pending"),
        "Cannot acknowledge nonterminal task"
    );
    if record.acknowledged_at == 0 {
        record.acknowledged_at = now();
        save_record(state, &record)?;
    }
    Ok(())
}
fn recover(state: &Path) -> Result<()> {
    for entry in std::fs::read_dir(state.join("journal"))? {
        let entry = entry?;
        if entry.path().extension().is_some_and(|s| s == "dpapi") {
            let mut record: Record = serde_json::from_slice(&storage::load(&entry.path())?)?;
            if matches!(record.status.as_str(), "running" | "approval_pending") {
                record.status = "uncertain".into();
                record.error =
                    "Agent restarted before outcome was committed; action will not be replayed"
                        .into();
                save_record(state, &record)?;
            }
        }
    }
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    let result = run().await;
    #[cfg(windows)]
    if let Err(error) = &result {
        let args = std::env::args().collect::<Vec<_>>();
        if args.len() == 1
            || args.iter().any(|a| {
                matches!(
                    a.as_str(),
                    "--ui" | "--background" | "--install" | "--upgrade" | "--uninstall"
                )
            })
        {
            use windows_sys::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
            let caption = "OpenCommander\0".encode_utf16().collect::<Vec<_>>();
            let text = format!("OpenCommander could not complete this operation.\n\n{error:#}\0")
                .encode_utf16()
                .collect::<Vec<_>>();
            unsafe {
                MessageBoxW(
                    std::ptr::null_mut(),
                    text.as_ptr(),
                    caption.as_ptr(),
                    MB_OK | MB_ICONERROR,
                );
            }
        }
    }
    result
}
async fn run() -> Result<()> {
    ensure!(cfg!(windows), "The native agent requires Windows");
    let cli = Cli::parse();
    let setup_name = std::env::current_exe()?.file_name().is_some_and(|p| {
        p.to_string_lossy()
            .eq_ignore_ascii_case("OpenCommanderSetup.exe")
    });
    if cli.install || (setup_name && std::env::args_os().len() == 1) {
        return installation::install(false, !cli.no_launch);
    }
    if cli.upgrade {
        return installation::install(true, !cli.no_launch);
    }
    if cli.uninstall {
        return installation::uninstall();
    }
    #[cfg(windows)]
    if cli.background {
        unsafe {
            windows_sys::Win32::System::Console::FreeConsole();
        }
    }
    let state = cli.state.unwrap_or_else(|| {
        PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into()))
            .join("OpenCommander")
    });
    std::fs::create_dir_all(state.join("journal"))?;
    std::fs::create_dir_all(state.join("output"))?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.join("agent.lock"))?;
    if !cli.status {
        lock.try_lock()
            .context("Another agent owns this state directory")?;
    }
    if cli.forget {
        if state.join("credentials.dpapi").exists() {
            std::fs::remove_file(state.join("credentials.dpapi"))?;
        }
        println!("Local identity removed. Revoke the device at the gateway as well.");
        return Ok(());
    }
    let native_ui = !cli.status && (cli.ui || cli.background || std::env::args_os().len() == 1);
    let control = Arc::new(ui::Control::default());
    if let Ok(mode) = storage::load(&state.join("access.dpapi")) {
        control
            .mode
            .store(mode.first().copied().unwrap_or(0).min(2), Ordering::SeqCst);
    }
    let cancels: Cancellations = Arc::new(Mutex::new(HashMap::new()));
    if native_ui && !state.join("config.json").exists() {
        let root = PathBuf::from(std::env::var("USERPROFILE")?)
            .join("Documents")
            .join("OpenCommander Workspace");
        policy::reject_links(&root)?;
        std::fs::create_dir_all(&root)?;
        let cfg = Config {
            gateway: "https://gateway-production-0964.up.railway.app".into(),
            policy: policy::Policy {
                roots: vec![root],
                protected_paths: vec![],
                applications: vec![],
                allow_all_applications: false,
                allow_modify: false,
                allow_screenshots: false,
                ask_before_sensitive: true,
            },
            dev: false,
        };
        std::fs::write(state.join("config.json"), serde_json::to_vec_pretty(&cfg)?)?;
    }
    let mut cfg: Config = if cli.init {
        ensure!(
            !state.join("credentials.dpapi").exists(),
            "Already paired; revoke and --forget before re-enrollment"
        );
        let mut cfg = Config {
            gateway: cli
                .gateway
                .context("--gateway is required for initialization")?,
            policy: policy::Policy {
                roots: cli.root,
                protected_paths: vec![],
                applications: cli.application,
                allow_all_applications: false,
                allow_modify: cli.allow_modify,
                allow_screenshots: cli.allow_screenshots,
                ask_before_sensitive: true,
            },
            dev: cli.dev,
        };
        validate_gateway(&cfg.gateway, cfg.dev)?;
        cfg.policy.validate()?;
        cfg.policy.protect_internal(&state)?;
        std::fs::write(state.join("config.json"), serde_json::to_vec_pretty(&cfg)?)?;
        cfg
    } else {
        serde_json::from_slice(
            &std::fs::read(state.join("config.json"))
                .context("Run --init to configure and pair first")?,
        )?
    };
    cfg.policy.validate()?;
    cfg.policy.protect_internal(&state)?;
    if native_ui {
        ui::start(
            state.clone(),
            control.clone(),
            cancels.clone(),
            !cli.background || !state.join("credentials.dpapi").exists(),
        );
    }
    let creds: Credentials = if cli.init {
        enroll(&state, &cfg, &control).await?
    } else if native_ui && !state.join("credentials.dpapi").exists() {
        control.pair.store(true, Ordering::SeqCst);
        loop {
            if control.shutdown.load(Ordering::SeqCst) {
                return Ok(());
            }
            if control.pair.swap(false, Ordering::SeqCst) {
                match enroll(&state, &cfg, &control).await {
                    Ok(creds) => break creds,
                    Err(error) => {
                        log_event(
                            &state,
                            json!({"event":"pairing_failed","error":error.to_string()}),
                        )?;
                        let _ = std::fs::remove_file(state.join("pairing-view.dpapi"));
                    }
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    } else {
        serde_json::from_slice(&storage::load(&state.join("credentials.dpapi"))?)?
    };
    if cli.status {
        let cached: Value = storage::load(&state.join("connection.dpapi"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or(Value::Null);
        let pid = cached["pid"].as_u64().unwrap_or(0);
        let system = sysinfo::System::new_all();
        let running = system
            .process(sysinfo::Pid::from_u32(pid as u32))
            .is_some_and(|p| {
                p.exe().and_then(|v| v.canonicalize().ok())
                    == std::env::current_exe()
                        .ok()
                        .and_then(|v| v.canonicalize().ok())
            });
        let connected = running
            && cached["connected"] == true
            && now().saturating_sub(cached["updated_at"].as_u64().unwrap_or(0)) < 45;
        println!(
            "{}",
            json!({"device_id":creds.device_id,"owner":creds.owner,"gateway":cfg.gateway,"roots":cfg.policy.roots,"applications":cfg.policy.applications,"allow_all_applications":cfg.policy.allow_all_applications,"permission_modify":cfg.policy.allow_modify,"permission_screenshots":cfg.policy.allow_screenshots,"running":running,"connected":connected,"last_state_at":cached["updated_at"]})
        );
        return Ok(());
    }
    history::prune(&state, now(), false)?;
    recover(&state)?;
    let scheduler = TaskScheduler::default();
    let mut failures = 0u32;
    loop {
        {
            let active = cancels
                .lock()
                .map_err(|_| anyhow::anyhow!("Cancellation lock failed"))?;
            history::prune(&state, now(), !active.is_empty())?;
        }
        consume_local_control(&state, &control)?;
        if control.shutdown.load(Ordering::SeqCst) {
            control.cancel_all(&cancels);
            connection_state(&state, false)?;
            return Ok(());
        }
        if control.mode.load(Ordering::SeqCst) == 2 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            continue;
        }
        let connected_since = std::time::Instant::now();
        match connect(
            &state,
            &cfg,
            &creds,
            cancels.clone(),
            scheduler.clone(),
            control.clone(),
        )
        .await
        {
            Ok(()) => {
                for flag in cancels
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Cancellation lock failed"))?
                    .values()
                {
                    flag.store(true, Ordering::SeqCst);
                }
                let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
                while !cancels
                    .lock()
                    .map_err(|_| anyhow::anyhow!("Cancellation lock failed"))?
                    .is_empty()
                    && tokio::time::Instant::now() < deadline
                {
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                connection_state(&state, false)?;
                return Ok(());
            }
            Err(e) => {
                connection_state(&state, false)?;
                log_event(
                    &state,
                    json!({"event":"reconnecting","reason":e.to_string()}),
                )?;
            }
        }
        if connected_since.elapsed() > Duration::from_secs(60) {
            failures = 0
        }
        failures = failures.saturating_add(1).min(6);
        let delay = (1u64 << failures) + u64::from(rand::random::<u8>()) % 3;
        tokio::select! {_=tokio::time::sleep(Duration::from_secs(delay))=>(),_=tokio::signal::ctrl_c()=>return Ok(())}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(windows)]
    #[tokio::test]
    async fn terminal_scheduler_does_not_block_a_second_command_behind_a_server() -> Result<()> {
        let root = tempfile::tempdir()?;
        let state = root.path().join("state");
        let workspace = root.path().join("workspace");
        std::fs::create_dir_all(state.join("journal"))?;
        std::fs::create_dir(&workspace)?;
        let config = Config {
            gateway: "http://127.0.0.1:8080".into(),
            dev: true,
            policy: policy::Policy {
                roots: vec![workspace.clone()],
                protected_paths: vec![],
                applications: vec![],
                allow_all_applications: false,
                allow_modify: true,
                allow_screenshots: false,
                ask_before_sensitive: false,
            },
        };
        std::fs::write(state.join("config.json"), serde_json::to_vec(&config)?)?;
        let marker = workspace.join("ready.txt");
        let (sender, _receiver) = mpsc::channel(16);
        let cancels = Arc::new(Mutex::new(HashMap::new()));
        let scheduler = TaskScheduler::default();
        let control = Arc::new(ui::Control::default());
        let runtime = || TaskRuntime {
            state: state.clone(),
            sender: sender.clone(),
            cancels: cancels.clone(),
            scheduler: scheduler.clone(),
            control: control.clone(),
            streaming: false,
        };
        let task = |id: &str, command: String, seconds: u64| Task {
            task_id: id.into(),
            owner: "alice".into(),
            device_id: "test-device".into(),
            action: "terminal_execute".into(),
            args: json!({"cwd":workspace,"command":command}),
            status: "dispatched".into(),
            category: "SENSITIVE".into(),
            created_at: now(),
            deadline: now() + seconds,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let long = tokio::spawn(run_task(
            task(
                "long-server",
                format!(
                    "[IO.File]::WriteAllText('{}','READY');Start-Sleep -Seconds 10",
                    marker.to_string_lossy().replace('\'', "''")
                ),
                20,
            ),
            cancel.clone(),
            runtime(),
        ));
        for _ in 0..100 {
            if marker.exists() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        ensure!(marker.exists(), "First terminal must have started");
        let second = run_task(
            task(
                "second-terminal",
                "[Console]::WriteLine('SECOND_DONE')".into(),
                3,
            ),
            Arc::new(AtomicBool::new(false)),
            runtime(),
        )
        .await;
        let concurrent = !long.is_finished();
        cancel.store(true, Ordering::SeqCst);
        long.await??;
        second?;
        let record: Record =
            serde_json::from_slice(&storage::load(&journal_path(&state, "second-terminal")?)?)?;
        ensure!(
            concurrent && record.status == "succeeded",
            "Second command must finish before the server"
        );
        for (id, args, code, program_succeeded) in [
            (
                "choice-result",
                json!({"cwd":workspace,"shell":"cmd","command":"choice /C YN /D Y /T 0"}),
                1,
                false,
            ),
            (
                "choice-expected",
                json!({"cwd":workspace,"shell":"cmd","command":"choice /C YN /D Y /T 0","success_exit_codes":[1,2]}),
                1,
                true,
            ),
            (
                "unexpected-exit",
                json!({"cwd":workspace,"shell":"cmd","command":"exit /b 7"}),
                7,
                false,
            ),
        ] {
            let mut example = task(id, String::new(), 5);
            example.args = args;
            run_task(example, Arc::new(AtomicBool::new(false)), runtime()).await?;
            let record: Record =
                serde_json::from_slice(&storage::load(&journal_path(&state, id)?)?)?;
            ensure!(
                record.status == "succeeded"
                    && record.error.is_empty()
                    && record.result["exit_code"] == code
                    && record.result["program_succeeded"] == program_succeeded,
                "Process completion was confused with its exit code"
            );
        }
        let mut rejected = task("launch-rejected", String::new(), 5);
        rejected.args = json!({"cwd":workspace,"shell":"invalid","command":"echo never"});
        run_task(rejected, Arc::new(AtomicBool::new(false)), runtime()).await?;
        let record: Record =
            serde_json::from_slice(&storage::load(&journal_path(&state, "launch-rejected")?)?)?;
        ensure!(record.status == "failed");
        Ok(())
    }
    #[test]
    fn signature_owner_expiry_and_recovery() -> Result<()> {
        let key = SigningKey::from_bytes(&[7; 32]);
        let creds = Credentials {
            device_id: "test-device".into(),
            owner: "alice".into(),
            seed: [3; 32],
            gateway_public_key: STANDARD.encode(key.verifying_key().as_bytes()),
        };
        let mut t = Task {
            task_id: "test-task".into(),
            owner: "alice".into(),
            device_id: "test-device".into(),
            action: "system_info".into(),
            args: json!({}),
            status: "dispatched".into(),
            category: "READ".into(),
            created_at: now(),
            deadline: now() + 60,
        };
        let envelope = |t: &Task| -> Result<Value> {
            let b = serde_json::to_vec(t)?;
            Ok(
                json!({"payload":STANDARD.encode(&b),"signature":STANDARD.encode(key.sign(&b).to_bytes())}),
            )
        };
        ensure!(verify_task(&envelope(&t)?, &creds).is_ok());
        t.owner = "mallory".into();
        ensure!(verify_task(&envelope(&t)?, &creds).is_err());
        t.owner = "alice".into();
        t.deadline = now() - 1;
        ensure!(verify_task(&envelope(&t)?, &creds).is_err());
        ensure!(validate_gateway("http://evil.example", true).is_err());
        ensure!(validate_gateway("http://example.com", false).is_err());
        ensure!(journal_path(Path::new("."), "../../escape").is_err());
        #[cfg(windows)]
        {
            let dir = tempfile::tempdir()?;
            std::fs::create_dir(dir.path().join("journal"))?;
            let r = Record {
                task_id: "test-task".into(),
                status: "running".into(),
                result: Value::Null,
                error: String::new(),
                fingerprint: "hash".into(),
                acknowledged_at: 0,
            };
            save_record(dir.path(), &r)?;
            recover(dir.path())?;
            let recovered: Record =
                serde_json::from_slice(&storage::load(&journal_path(dir.path(), "test-task")?)?)?;
            ensure!(recovered.status == "uncertain");
        }
        Ok(())
    }
    #[cfg(windows)]
    #[test]
    fn journal_ack_requires_matching_terminal_outcome() -> Result<()> {
        let dir = tempfile::tempdir()?;
        std::fs::create_dir(dir.path().join("journal"))?;
        let mut record = Record {
            task_id: "ack-fixture".into(),
            status: "running".into(),
            result: Value::Null,
            error: String::new(),
            fingerprint: "hash".into(),
            acknowledged_at: 0,
        };
        save_record(dir.path(), &record)?;
        ensure!(
            acknowledge(
                dir.path(),
                &json!({"task_id":record.task_id,"status":"running"})
            )
            .is_err()
        );
        record.status = "succeeded".into();
        save_record(dir.path(), &record)?;
        ensure!(
            acknowledge(
                dir.path(),
                &json!({"task_id":record.task_id,"status":"failed"})
            )
            .is_err()
        );
        acknowledge(
            dir.path(),
            &json!({"task_id":record.task_id,"status":"succeeded"}),
        )?;
        let saved: Record =
            serde_json::from_slice(&storage::load(&journal_path(dir.path(), &record.task_id)?)?)?;
        ensure!(saved.acknowledged_at > 0);
        acknowledge(
            dir.path(),
            &json!({"task_id":record.task_id,"status":"succeeded"}),
        )?;
        ensure!(saved.fingerprint == record.fingerprint);
        Ok(())
    }
}
