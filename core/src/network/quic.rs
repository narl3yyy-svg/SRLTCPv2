//! QUIC transport via quinn for LAN/WAN P2P connections.

use std::net::SocketAddr;
use std::sync::Arc;
use quinn::{ClientConfig, Connection, Endpoint, ServerConfig};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;

#[derive(Debug, Error)]
pub enum QuicError {
    #[error("endpoint error: {0}")]
    Endpoint(String),
    #[error("connection error: {0}")]
    Connection(String),
    #[error("not running")]
    NotRunning,
}

/// QUIC endpoint wrapper with graceful lifecycle.
pub struct QuicTransport {
    endpoint: Option<Endpoint>,
    connections: Arc<RwLock<Vec<Connection>>>,
    listen_addr: Option<SocketAddr>,
}

impl QuicTransport {
    pub fn new() -> Self {
        Self {
            endpoint: None,
            connections: Arc::new(RwLock::new(Vec::new())),
            listen_addr: None,
        }
    }

    /// Start listening on the given address.
    pub async fn listen(&mut self, addr: SocketAddr) -> Result<(), QuicError> {
        let (cert, key) = generate_self_signed_cert()
            .map_err(|e| QuicError::Endpoint(e.to_string()))?;

        let server_crypto = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![cert], rustls::pki_types::PrivateKeyDer::Pkcs8(key))
            .map_err(|e| QuicError::Endpoint(e.to_string()))?;

        let server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)
                .map_err(|e| QuicError::Endpoint(e.to_string()))?,
        ));

        let endpoint = Endpoint::server(server_config, addr)
            .map_err(|e| QuicError::Endpoint(e.to_string()))?;

        info!(%addr, "QUIC listener started");
        self.endpoint = Some(endpoint);
        self.listen_addr = Some(addr);
        Ok(())
    }

    /// Connect to a remote peer.
    pub async fn connect(&self, addr: SocketAddr) -> Result<Connection, QuicError> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or(QuicError::NotRunning)?;

        let client_crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();

        let client_config = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)
                .map_err(|e| QuicError::Connection(e.to_string()))?,
        ));

        let conn = endpoint
            .connect_with(client_config, addr, "srltcp")
            .map_err(|e| QuicError::Connection(e.to_string()))?
            .await
            .map_err(|e| QuicError::Connection(e.to_string()))?;

        self.connections.write().await.push(conn.clone());
        info!(%addr, "QUIC connected");
        Ok(conn)
    }

    /// Accept an incoming connection.
    pub async fn accept(&self) -> Result<Connection, QuicError> {
        let endpoint = self
            .endpoint
            .as_ref()
            .ok_or(QuicError::NotRunning)?;

        let incoming = endpoint
            .accept()
            .await
            .ok_or(QuicError::NotRunning)?;

        let conn = incoming
            .await
            .map_err(|e| QuicError::Connection(e.to_string()))?;

        self.connections.write().await.push(conn.clone());
        Ok(conn)
    }

    /// Gracefully close all connections and the endpoint.
    pub async fn shutdown(&mut self) {
        let mut conns = self.connections.write().await;
        for conn in conns.drain(..) {
            conn.close(0u32.into(), b"shutdown");
        }

        if let Some(endpoint) = self.endpoint.take() {
            endpoint.close(0u32.into(), b"shutdown");
            endpoint.wait_idle().await;
            info!("QUIC endpoint shut down");
        }
    }

    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.listen_addr
    }
}

fn generate_self_signed_cert() -> Result<(CertificateDer<'static>, PrivatePkcs8KeyDer<'static>), String> {
    let cert = rcgen::generate_simple_self_signed(vec!["srltcp".into()])
        .map_err(|e| e.to_string())?;
    let cert_der = CertificateDer::from(cert.cert);
    let key_der = PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der());
    Ok((cert_der, key_der))
}

/// Skip TLS verification for P2P (identity verified via Noise/Ed25519 instead).
#[derive(Debug)]
struct SkipServerVerification;

impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &CertificateDer<'_>,
        _dss: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        vec![
            rustls::SignatureScheme::ED25519,
            rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            rustls::SignatureScheme::RSA_PSS_SHA256,
        ]
    }
}