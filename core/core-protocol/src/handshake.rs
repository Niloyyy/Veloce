use std::fmt;

use crate::error::{ProtocolError, Result};
use crate::messages::ControlMessage;
use crate::types::{NodeId, TransferManifest, PROTOCOL_VERSION};

/// Role played by the local endpoint in a transfer session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionRole {
    Sender,
    Receiver,
}

/// Explicit states of the control-stream handshake and transfer lifecycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandshakeState {
    Init,
    HelloExchanged {
        peer_node_id: NodeId,
        peer_device_name: String,
    },
    Offered {
        peer_node_id: NodeId,
        peer_device_name: String,
        manifest: Box<TransferManifest>,
    },
    Transferring {
        peer_node_id: NodeId,
        peer_device_name: String,
        manifest: Box<TransferManifest>,
        resume_bitfield: Vec<u8>,
    },
    Completed {
        verified_chunks: u32,
    },
    Rejected {
        reason: String,
    },
    Cancelled {
        reason: String,
    },
}

impl fmt::Display for HandshakeState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HandshakeState::Init => write!(f, "Init"),
            HandshakeState::HelloExchanged { .. } => write!(f, "HelloExchanged"),
            HandshakeState::Offered { .. } => write!(f, "Offered"),
            HandshakeState::Transferring { .. } => write!(f, "Transferring"),
            HandshakeState::Completed { .. } => write!(f, "Completed"),
            HandshakeState::Rejected { .. } => write!(f, "Rejected"),
            HandshakeState::Cancelled { .. } => write!(f, "Cancelled"),
        }
    }
}

/// Deterministic handshake state machine validating all control messages and transitions.
#[derive(Debug, Clone)]
pub struct HandshakeStateMachine {
    role: SessionRole,
    expected_token: Option<[u8; 16]>,
    local_hello_sent: bool,
    state: HandshakeState,
}

impl HandshakeStateMachine {
    pub fn new(role: SessionRole, expected_token: Option<[u8; 16]>) -> Self {
        Self {
            role,
            expected_token,
            local_hello_sent: false,
            state: HandshakeState::Init,
        }
    }

    pub fn role(&self) -> SessionRole {
        self.role
    }

    pub fn state(&self) -> &HandshakeState {
        &self.state
    }

    /// Processes an incoming control message from the remote peer and advances the state machine.
    pub fn on_receive(&mut self, msg: ControlMessage) -> Result<&HandshakeState> {
        match msg {
            ControlMessage::Ping(_) | ControlMessage::Pong(_) => return Ok(&self.state),
            ControlMessage::Cancel { reason } => {
                if matches!(
                    self.state,
                    HandshakeState::Completed { .. }
                        | HandshakeState::Rejected { .. }
                        | HandshakeState::Cancelled { .. }
                ) {
                    return Err(ProtocolError::InvalidTransition {
                        state: self.state.to_string(),
                        event: "Cancel".into(),
                    });
                }
                self.state = HandshakeState::Cancelled { reason };
                return Ok(&self.state);
            }
            _ => {}
        }

        match (&self.state, self.role, msg) {
            (
                HandshakeState::Init,
                _,
                ControlMessage::Hello {
                    protocol_version,
                    node_id,
                    device_name,
                    session_token,
                },
            ) => {
                if protocol_version != PROTOCOL_VERSION {
                    return Err(ProtocolError::UnsupportedVersion {
                        expected: PROTOCOL_VERSION,
                        received: protocol_version,
                    });
                }
                if let Some(expected) = self.expected_token {
                    if session_token != Some(expected) {
                        return Err(ProtocolError::SessionTokenMismatch);
                    }
                }
                self.state = HandshakeState::HelloExchanged {
                    peer_node_id: node_id,
                    peer_device_name: device_name,
                };
                Ok(&self.state)
            }
            (
                HandshakeState::HelloExchanged {
                    peer_node_id,
                    peer_device_name,
                },
                SessionRole::Receiver,
                ControlMessage::Offer { manifest, .. },
            ) => {
                manifest.validate()?;
                self.state = HandshakeState::Offered {
                    peer_node_id: *peer_node_id,
                    peer_device_name: peer_device_name.clone(),
                    manifest: Box::new(manifest),
                };
                Ok(&self.state)
            }
            (
                HandshakeState::Offered {
                    peer_node_id,
                    peer_device_name,
                    manifest,
                },
                SessionRole::Sender,
                ControlMessage::Accept { resume_bitfield },
            ) => {
                let expected_bytes = (manifest.total_chunks as usize).div_ceil(8);
                if !resume_bitfield.is_empty() && resume_bitfield.len() != expected_bytes {
                    return Err(ProtocolError::InvalidManifest(format!(
                        "resume_bitfield length {} does not match expected {} bytes",
                        resume_bitfield.len(),
                        expected_bytes
                    )));
                }
                self.state = HandshakeState::Transferring {
                    peer_node_id: *peer_node_id,
                    peer_device_name: peer_device_name.clone(),
                    manifest: manifest.clone(),
                    resume_bitfield,
                };
                Ok(&self.state)
            }
            (
                HandshakeState::Offered { .. },
                SessionRole::Sender,
                ControlMessage::Reject { reason },
            ) => {
                self.state = HandshakeState::Rejected { reason };
                Ok(&self.state)
            }
            (
                HandshakeState::Transferring { manifest, .. },
                SessionRole::Sender,
                ControlMessage::TransferComplete { verified_chunks },
            ) => {
                if verified_chunks != manifest.total_chunks {
                    return Err(ProtocolError::InvalidManifest(format!(
                        "TransferComplete reported {} chunks, expected {}",
                        verified_chunks, manifest.total_chunks
                    )));
                }
                self.state = HandshakeState::Completed { verified_chunks };
                Ok(&self.state)
            }
            (state, _, other) => Err(ProtocolError::InvalidTransition {
                state: state.to_string(),
                event: format!("{other:?}"),
            }),
        }
    }

    /// Records a locally sent control message so both sides stay synchronized.
    pub fn on_send(&mut self, msg: &ControlMessage) -> Result<&HandshakeState> {
        match msg {
            ControlMessage::Ping(_) | ControlMessage::Pong(_) => return Ok(&self.state),
            ControlMessage::Cancel { reason } => {
                self.state = HandshakeState::Cancelled {
                    reason: reason.clone(),
                };
                return Ok(&self.state);
            }
            _ => {}
        }

        match (&self.state, self.role, msg) {
            (HandshakeState::Init | HandshakeState::HelloExchanged { .. }, _, ControlMessage::Hello { .. })
                if !self.local_hello_sent =>
            {
                self.local_hello_sent = true;
                Ok(&self.state)
            }
            (
                HandshakeState::HelloExchanged {
                    peer_node_id,
                    peer_device_name,
                },
                SessionRole::Sender,
                ControlMessage::Offer { manifest, .. },
            ) => {
                manifest.validate()?;
                self.state = HandshakeState::Offered {
                    peer_node_id: *peer_node_id,
                    peer_device_name: peer_device_name.clone(),
                    manifest: Box::new(manifest.clone()),
                };
                Ok(&self.state)
            }
            (
                HandshakeState::Offered {
                    peer_node_id,
                    peer_device_name,
                    manifest,
                },
                SessionRole::Receiver,
                ControlMessage::Accept { resume_bitfield },
            ) => {
                self.state = HandshakeState::Transferring {
                    peer_node_id: *peer_node_id,
                    peer_device_name: peer_device_name.clone(),
                    manifest: manifest.clone(),
                    resume_bitfield: resume_bitfield.clone(),
                };
                Ok(&self.state)
            }
            (
                HandshakeState::Offered { .. },
                SessionRole::Receiver,
                ControlMessage::Reject { reason },
            ) => {
                self.state = HandshakeState::Rejected {
                    reason: reason.clone(),
                };
                Ok(&self.state)
            }
            (
                HandshakeState::Transferring { manifest, .. },
                SessionRole::Receiver,
                ControlMessage::TransferComplete { verified_chunks },
            ) => {
                if *verified_chunks != manifest.total_chunks {
                    return Err(ProtocolError::InvalidManifest(format!(
                        "cannot send TransferComplete with {} / {} chunks",
                        verified_chunks, manifest.total_chunks
                    )));
                }
                self.state = HandshakeState::Completed {
                    verified_chunks: *verified_chunks,
                };
                Ok(&self.state)
            }
            (state, _, other) => Err(ProtocolError::InvalidTransition {
                state: state.to_string(),
                event: format!("send({other:?})"),
            }),
        }
    }
}
