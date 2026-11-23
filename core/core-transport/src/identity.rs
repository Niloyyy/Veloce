use std::sync::Arc;
use std::time::Duration;

use core_protocol::{NodeId, ALPN_VELOCE};
use quinn::congestion::BbrConfig;
use quinn::crypto::rustls::{QuicClientConfig, QuicServerConfig};
use quinn::{ClientConfig, IdleTimeout, ServerConfig, TransportConfig};
use rcgen::{CertificateParams, DistinguishedName, DnType, KeyPair, PKCS_ED25519};
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::ring::default_provider;
use rustls::crypto::{verify_tls12_signature, verify_tls13_signature, WebPkiSupportedAlgorithms};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{DigitallySignedStruct, DistinguishedName as RustlsDn, Error as TlsError, SignatureScheme};
use x509_parser::oid_registry::OID_SIG_ED25519;
use x509_parser::prelude::*;

use crate::error::{Result, TransportError};

/// Ephemeral Ed25519 peer identity backing self-signed TLS 1.3 QUIC authentication.
///
/// The peer's `NodeId` is its raw 32-byte Ed25519 public key.
#[derive(Clone)]
pub struct PeerIdentity {
    node_id: NodeId,
    cert_der: CertificateDer<'static>,
    key_der: Vec<u8>,
}

impl PeerIdentity {
    /// Generates a fresh ephemeral Ed25519 keypair and self-signed X.509 certificate.
    pub fn generate() -> Result<Self> {
        let key_pair = KeyPair::generate_for(&PKCS_ED25519)
            .map_err(|e| TransportError::Tls(format!("ed25519 keygen failed: {e}")))?;

        let pub_raw = key_pair.public_key_raw();
        if pub_raw.len() != 32 {
            return Err(TransportError::Tls(format!(
                "unexpected Ed25519 public key length {}",
                pub_raw.len()
            )));
        }
        let mut node_bytes = [0u8; 32];
        node_bytes.copy_from_slice(pub_raw);
        let node_id = NodeId::from_bytes(node_bytes);

        let mut params = CertificateParams::new(vec![format!("{}.veloce.local", node_id.to_base32())])
            .map_err(|e| TransportError::Tls(format!("cert params error: {e}")))?;
        let mut dn = DistinguishedName::new();
        dn.push(DnType::CommonName, node_id.to_base32());
        params.distinguished_name = dn;

        let cert = params
            .self_signed(&key_pair)
            .map_err(|e| TransportError::Tls(format!("self-signed cert generation failed: {e}")))?;

        Ok(Self {
            node_id,
            cert_der: CertificateDer::from(cert.der().to_vec()),
            key_der: key_pair.serialize_der(),
        })
    }

    pub fn node_id(&self) -> NodeId {
        self.node_id
    }

    pub fn cert_der(&self) -> CertificateDer<'static> {
        self.cert_der.clone()
    }

    pub fn private_key_der(&self) -> PrivateKeyDer<'static> {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(self.key_der.clone()))
    }

    /// Builds a QUIC `ServerConfig` enforcing mutual TLS 1.3 with Ed25519 peer identities and BBR.
    pub fn build_server_config(&self, transport: Arc<TransportConfig>) -> Result<ServerConfig> {
        let provider = Arc::new(default_provider());
        let verifier = Arc::new(Ed25519PeerVerifier::new(None, provider.signature_verification_algorithms));

        let mut tls = rustls::ServerConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| TransportError::Tls(e.to_string()))?
            .with_client_cert_verifier(verifier)
            .with_single_cert(vec![self.cert_der()], self.private_key_der())
            .map_err(|e| TransportError::Tls(e.to_string()))?;

        tls.alpn_protocols = vec![ALPN_VELOCE.to_vec()];
        tls.max_early_data_size = 0;

        let quic_crypto = QuicServerConfig::try_from(tls)
            .map_err(|e| TransportError::Tls(e.to_string()))?;
        let mut server_config = ServerConfig::with_crypto(Arc::new(quic_crypto));
        server_config.transport_config(transport);
        Ok(server_config)
    }

    /// Builds a QUIC `ClientConfig` enforcing TLS 1.3 mutual authentication and BBR.
    ///
    /// When `expected_peer` is `Some(node_id)`, the TLS handshake aborts unless the remote peer
    /// proves possession of the private key corresponding to `node_id`.
    pub fn build_client_config(
        &self,
        expected_peer: Option<NodeId>,
        transport: Arc<TransportConfig>,
    ) -> Result<ClientConfig> {
        let provider = Arc::new(default_provider());
        let verifier = Arc::new(Ed25519PeerVerifier::new(
            expected_peer,
            provider.signature_verification_algorithms,
        ));

        let mut tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_protocol_versions(&[&rustls::version::TLS13])
            .map_err(|e| TransportError::Tls(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(verifier)
            .with_client_auth_cert(vec![self.cert_der()], self.private_key_der())
            .map_err(|e| TransportError::Tls(e.to_string()))?;

        tls.alpn_protocols = vec![ALPN_VELOCE.to_vec()];

        let quic_crypto = QuicClientConfig::try_from(tls)
            .map_err(|e| TransportError::Tls(e.to_string()))?;
        let mut client_config = ClientConfig::new(Arc::new(quic_crypto));
        client_config.transport_config(transport);
        Ok(client_config)
    }
}

/// Constructs a high-throughput QUIC `TransportConfig` using BBR congestion control and
/// large flow-control windows tuned for Long Fat Networks (high bandwidth-delay product).
pub fn build_bbr_transport_config() -> Result<Arc<TransportConfig>> {
    let mut cfg = TransportConfig::default();
    let mut bbr = BbrConfig::default();
    // Use a 512 KiB initial congestion window so BBR saturates gigabit LANs and
    // high-BDP WAN links quickly instead of spending extra RTTs ramping from 12 KiB.
    bbr.initial_window(512 * 1024);
    cfg.congestion_controller_factory(Arc::new(bbr));
    cfg.max_concurrent_bidi_streams(128u32.into());
    cfg.max_concurrent_uni_streams(64u32.into());
    // 64 MiB connection window & 16 MiB per-stream window prevent flow-control starvation on LFNs.
    cfg.receive_window((64u32 * 1024 * 1024).into());
    cfg.stream_receive_window((16u32 * 1024 * 1024).into());
    cfg.send_window(64 * 1024 * 1024);
    // 1380-byte initial MTU maximizes payload efficiency while still leaving headroom for
    // RelayForward framing (~76B) inside standard 1500-byte Ethernet MTU without fragmentation.
    cfg.initial_mtu(1380);
    cfg.keep_alive_interval(Some(Duration::from_secs(5)));
    let idle = IdleTimeout::try_from(Duration::from_secs(45))
        .map_err(|e| TransportError::Tls(format!("invalid idle timeout: {e}")))?;
    cfg.max_idle_timeout(Some(idle));
    Ok(Arc::new(cfg))
}

/// Extracts the authenticated `NodeId` from a peer's DER-encoded Ed25519 X.509 certificate.
pub fn extract_node_id_from_cert(cert: &CertificateDer<'_>) -> std::result::Result<NodeId, TlsError> {
    let (_, parsed) = parse_x509_certificate(cert.as_ref())
        .map_err(|_| TlsError::InvalidCertificate(rustls::CertificateError::BadEncoding))?;

    let spki = parsed.public_key();
    if spki.algorithm.algorithm != OID_SIG_ED25519 {
        return Err(TlsError::InvalidCertificate(
            rustls::CertificateError::BadSignature,
        ));
    }
    let raw_key = spki.subject_public_key.data.as_ref();
    if raw_key.len() != 32 {
        return Err(TlsError::InvalidCertificate(
            rustls::CertificateError::BadEncoding,
        ));
    }
    let mut arr = [0u8; 32];
    arr.copy_from_slice(raw_key);
    Ok(NodeId::from_bytes(arr))
}

/// Extracts the verified peer `NodeId` from an established Quinn connection.
pub fn peer_node_id(conn: &quinn::Connection) -> Result<NodeId> {
    let identity = conn
        .peer_identity()
        .ok_or_else(|| TransportError::Tls("peer did not present TLS certificate".into()))?;
    let certs = identity
        .downcast::<Vec<CertificateDer<'static>>>()
        .map_err(|_| TransportError::Tls("unexpected peer identity type".into()))?;
    let leaf = certs
        .first()
        .ok_or_else(|| TransportError::Tls("empty peer certificate chain".into()))?;
    extract_node_id_from_cert(leaf).map_err(|e| TransportError::Tls(e.to_string()))
}

#[derive(Debug)]
struct Ed25519PeerVerifier {
    expected_peer: Option<NodeId>,
    algs: WebPkiSupportedAlgorithms,
}

impl Ed25519PeerVerifier {
    fn new(expected_peer: Option<NodeId>, algs: WebPkiSupportedAlgorithms) -> Self {
        Self {
            expected_peer,
            algs,
        }
    }

    fn verify_peer_cert(&self, end_entity: &CertificateDer<'_>) -> std::result::Result<(), TlsError> {
        let actual = extract_node_id_from_cert(end_entity)?;
        if let Some(expected) = self.expected_peer {
            if actual != expected {
                return Err(TlsError::InvalidCertificate(
                    rustls::CertificateError::NotValidForName,
                ));
            }
        }
        Ok(())
    }
}

impl ServerCertVerifier for Ed25519PeerVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, TlsError> {
        self.verify_peer_cert(end_entity)?;
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }
}

impl ClientCertVerifier for Ed25519PeerVerifier {
    fn root_hint_subjects(&self) -> &[RustlsDn] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> std::result::Result<ClientCertVerified, TlsError> {
        self.verify_peer_cert(end_entity)?;
        Ok(ClientCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls12_signature(message, cert, dss, &self.algs)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, TlsError> {
        verify_tls13_signature(message, cert, dss, &self.algs)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        vec![SignatureScheme::ED25519]
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }
}
