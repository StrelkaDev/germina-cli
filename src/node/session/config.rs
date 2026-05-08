use anyhow::{Context, anyhow};
use std::sync::{Arc, OnceLock};

struct GeneratedCert {
    cert_der: Vec<u8>,
    key_der: Vec<u8>,
}

static CERT: OnceLock<GeneratedCert> = OnceLock::new();

fn get_or_generate_cert() -> anyhow::Result<&'static GeneratedCert> {
    if let Some(cert) = CERT.get() {
        return Ok(cert);
    }

    let rcgen::CertifiedKey { cert, signing_key } =
        rcgen::generate_simple_self_signed(vec!["germina-orchestrator".to_string()])
            .context("Failed to generate self-signed certificate")?;

    let generated = GeneratedCert {
        cert_der: cert.der().to_vec(),
        key_der: signing_key.serialize_der(),
    };

    Ok(CERT.get_or_init(|| generated))
}

pub(crate) fn server_config() -> anyhow::Result<quinn::ServerConfig> {
    let generated = get_or_generate_cert()?;
    let cert = rustls::pki_types::CertificateDer::from(generated.cert_der.clone());
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        generated.key_der.clone(),
    ));

    let mut config = quinn::ServerConfig::with_single_cert(vec![cert], key)
        .context("Failed to create QUIC server config")?;
    config.transport = Arc::new(quinn::TransportConfig::default());
    Ok(config)
}

pub(crate) fn client_config() -> anyhow::Result<quinn::ClientConfig> {
    let generated = get_or_generate_cert()?;
    let cert = rustls::pki_types::CertificateDer::from(generated.cert_der.clone());
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
