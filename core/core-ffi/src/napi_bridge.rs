use std::sync::{Arc, RwLock};

use napi::bindgen_prelude::*;
use napi::threadsafe_function::{ErrorStrategy, ThreadsafeFunction, ThreadsafeFunctionCallMode};
use napi_derive::napi;
use tokio::sync::broadcast;

use crate::engine::{EngineConfigDto, VeloceEngine};

static GLOBAL_ENGINE: RwLock<Option<Arc<VeloceEngine>>> = RwLock::new(None);

fn to_napi_err(err: impl std::fmt::Display) -> Error {
    Error::from_reason(err.to_string())
}

/// Initialization options passed from Electron / Node.js.
#[napi(object)]
#[derive(Debug, Clone)]
pub struct JsEngineConfig {
    pub device_name: String,
    pub bind_addr: Option<String>,
    pub enable_mdns: Option<bool>,
    pub parallel_streams: Option<u32>,
    pub relay_addrs: Option<Vec<String>>,
    pub profile: Option<String>,
    pub override_lan_addrs: Option<Vec<String>>,
    pub force_relay: Option<bool>,
    pub identity_key_path: Option<String>,
}

impl From<JsEngineConfig> for EngineConfigDto {
    fn from(cfg: JsEngineConfig) -> Self {
        let profile = cfg.profile.as_deref().map(|s| {
            if s.eq_ignore_ascii_case("mobile") {
                core_protocol::TransferProfile::Mobile
            } else {
                core_protocol::TransferProfile::Desktop
            }
        });
        Self {
            device_name: cfg.device_name,
            bind_addr: cfg.bind_addr,
            enable_mdns: cfg.enable_mdns,
            parallel_streams: cfg.parallel_streams,
            relay_addrs: cfg.relay_addrs,
            profile,
            override_lan_addrs: cfg.override_lan_addrs,
            force_relay: cfg.force_relay,
            identity_key_path: cfg.identity_key_path,
        }
    }
}

/// Multi-instance native Veloce handle exposed to Node.js / Electron via napi-rs.
#[napi]
pub struct VeloceNativeClient {
    engine: Arc<VeloceEngine>,
}

#[napi]
impl VeloceNativeClient {
    /// Initializes a new Veloce QUIC/BBR P2P engine instance.
    #[napi(factory)]
    pub async fn create(config: JsEngineConfig) -> Result<Self> {
        let engine = VeloceEngine::init(config.into())
            .await
            .map_err(to_napi_err)?;
        Ok(Self { engine })
    }

    /// Returns JSON-serialized local node metadata (`nodeId`, `shortId`, `deviceName`, `listenAddr`, `listenPort`).
    #[napi]
    pub fn node_info_json(&self) -> Result<String> {
        serde_json::to_string(&self.engine.node_info()).map_err(to_napi_err)
    }

    /// Registers a non-blocking JS callback receiving JSON-serialized `EngineEvent` payloads
    /// (`TransferProgress`, `PeerDiscovered`, `IncomingOffer`, `ConnectionTypeChanged`, etc.).
    #[napi]
    pub fn on_event(
        &self,
        #[napi(ts_arg_type = "(eventJson: string) => void")] callback: JsFunction,
    ) -> Result<()> {
        let tsfn: ThreadsafeFunction<String, ErrorStrategy::Fatal> =
            callback.create_threadsafe_function(256, |ctx| {
                let s: String = ctx.value;
                Ok(vec![ctx.env.create_string(&s)?])
            })?;

        let mut rx = self.engine.subscribe_events();
        napi::tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(evt) => {
                        if let Ok(json) = serde_json::to_string(&evt) {
                            let status =
                                tsfn.call(json, ThreadsafeFunctionCallMode::NonBlocking);
                            if status != Status::Ok {
                                break;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
        });
        Ok(())
    }

    /// Indexes files/folders and starts a hosted session (returns JSON `StartSessionResultDto`).
    #[napi]
    pub async fn start_session(
        &self,
        paths: Vec<String>,
        target_peer_node_id: Option<String>,
        relay_hints: Option<Vec<String>>,
    ) -> Result<String> {
        let res = self
            .engine
            .start_session(paths, target_peer_node_id, relay_hints.unwrap_or_default())
            .await
            .map_err(to_napi_err)?;
        serde_json::to_string(&res).map_err(to_napi_err)
    }

    /// Joins a remote session using a `veloce1...` ticket string (returns JSON `JoinSessionResultDto`).
    #[napi]
    pub async fn join_session(
        &self,
        ticket_code: String,
        output_dir: String,
        parallel_streams: Option<u32>,
    ) -> Result<String> {
        let res = self
            .engine
            .join_session(ticket_code, output_dir, parallel_streams)
            .await
            .map_err(to_napi_err)?;
        serde_json::to_string(&res).map_err(to_napi_err)
    }

    /// Lists nearby mDNS-discovered LAN peers (returns JSON array of `PeerDto`).
    #[napi]
    pub fn list_local_peers(&self) -> Result<String> {
        serde_json::to_string(&self.engine.list_local_peers()).map_err(to_napi_err)
    }

    /// Accepts an incoming LAN transfer offer and writes verified files into `output_dir`.
    #[napi]
    pub fn accept_incoming_transfer(&self, transfer_id: String, output_dir: String) -> Result<()> {
        self.engine
            .accept_incoming_transfer(&transfer_id, output_dir)
            .map_err(to_napi_err)
    }

    /// Rejects an incoming LAN transfer offer.
    #[napi]
    pub fn reject_incoming_transfer(
        &self,
        transfer_id: String,
        reason: Option<String>,
    ) -> Result<()> {
        self.engine
            .reject_incoming_transfer(&transfer_id, reason)
            .map_err(to_napi_err)
    }

    /// Pauses an active transfer while preserving verified chunks for later resume.
    #[napi]
    pub fn pause_transfer(&self, transfer_id: String) -> bool {
        self.engine.pause_transfer(&transfer_id)
    }

    /// Resumes a previously paused transfer from its saved `.veloce-resume-*.state` bitfield.
    #[napi]
    pub async fn resume_transfer(&self, transfer_id: String) -> Result<bool> {
        self.engine
            .resume_transfer(&transfer_id)
            .await
            .map_err(to_napi_err)
    }

    /// Cancels an active or paused transfer by ID.
    #[napi]
    pub fn cancel_transfer(&self, transfer_id: String) -> bool {
        self.engine.cancel_transfer(&transfer_id)
    }

    /// Cleanly shuts down the QUIC endpoint, mDNS daemon, and background tasks.
    #[napi]
    pub fn shutdown(&self) {
        self.engine.shutdown();
    }
}

/// Validates and decodes a `veloce1...` ticket into JSON `TicketPreviewDto` without network access.
#[napi]
pub fn parse_ticket(ticket_code: String) -> Result<String> {
    let preview = VeloceEngine::parse_ticket(&ticket_code).map_err(to_napi_err)?;
    serde_json::to_string(&preview).map_err(to_napi_err)
}

/// Singleton helper: initializes the global engine for simple app integrations.
#[napi]
pub async fn init_engine(config: JsEngineConfig) -> Result<String> {
    let engine = VeloceEngine::init(config.into())
        .await
        .map_err(to_napi_err)?;
    let info = serde_json::to_string(&engine.node_info()).map_err(to_napi_err)?;
    *GLOBAL_ENGINE.write().expect("global engine lock poisoned") = Some(engine);
    Ok(info)
}

fn global_engine() -> Result<Arc<VeloceEngine>> {
    GLOBAL_ENGINE
        .read()
        .expect("global engine lock poisoned")
        .clone()
        .ok_or_else(|| Error::from_reason("Veloce engine not initialized; call initEngine() first"))
}

#[napi]
pub async fn start_session(
    paths: Vec<String>,
    target_peer_node_id: Option<String>,
    relay_hints: Option<Vec<String>>,
) -> Result<String> {
    let engine = global_engine()?;
    let res = engine
        .start_session(paths, target_peer_node_id, relay_hints.unwrap_or_default())
        .await
        .map_err(to_napi_err)?;
    serde_json::to_string(&res).map_err(to_napi_err)
}

#[napi]
pub async fn join_session(
    ticket_code: String,
    output_dir: String,
    parallel_streams: Option<u32>,
) -> Result<String> {
    let engine = global_engine()?;
    let res = engine
        .join_session(ticket_code, output_dir, parallel_streams)
        .await
        .map_err(to_napi_err)?;
    serde_json::to_string(&res).map_err(to_napi_err)
}

#[napi]
pub fn list_local_peers() -> Result<String> {
    let engine = global_engine()?;
    serde_json::to_string(&engine.list_local_peers()).map_err(to_napi_err)
}

#[napi]
pub fn pause_transfer(transfer_id: String) -> Result<bool> {
    let engine = global_engine()?;
    Ok(engine.pause_transfer(&transfer_id))
}

#[napi]
pub async fn resume_transfer(transfer_id: String) -> Result<bool> {
    let engine = global_engine()?;
    engine
        .resume_transfer(&transfer_id)
        .await
        .map_err(to_napi_err)
}

#[napi]
pub fn cancel_transfer(transfer_id: String) -> Result<bool> {
    let engine = global_engine()?;
    Ok(engine.cancel_transfer(&transfer_id))
}

#[napi]
pub fn shutdown_engine() {
    if let Some(engine) = GLOBAL_ENGINE
        .write()
        .expect("global engine lock poisoned")
        .take()
    {
        engine.shutdown();
    }
}
