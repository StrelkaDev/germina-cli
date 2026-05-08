use anyhow::{Context, anyhow};
use std::sync::Arc;

const CERT_DER: &[u8] = include_bytes!("certs/orchestrator_cert.der");
const KEY_DER: &[u8] = include_bytes!("certs/orchestrator_key.der");

pub(crate) fn server_config() -> anyhow::Result<quinn::ServerConfig> {
    let cert = rustls::pki_types::CertificateDer::from(CERT_DER.to_vec());
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        KEY_DER.to_vec(),
    ));

    let mut config = quinn::ServerConfig::with_single_cert(vec![cert], key)
        .context("Failed to create QUIC server config")?;
    config.transport = Arc::new(quinn::TransportConfig::default());
    Ok(config)
}

pub(crate) fn client_config() -> anyhow::Result<quinn::ClientConfig> {
    let cert = rustls::pki_types::CertificateDer::from(CERT_DER.to_vec());
    let mut roots = rustls::RootCertStore::empty();
    roots
        .add(cert)
        .map_err(|e| anyhow!("Failed to add root cert: {e}"))?;

    let crypto = rustls::ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();

    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
        .map_err(|e| anyhow!("Failed to build QUIC client crypto: {e}"))?;

    Ok(quinn::ClientConfig::new(Arc::new(quic_crypto)))
}
