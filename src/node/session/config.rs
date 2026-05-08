use anyhow::{Context, anyhow};
use std::sync::{Arc, Mutex};

static RUNTIME_TLS_IDENTITY: Mutex<Option<(Vec<u8>, Vec<u8>)>> = Mutex::new(None);

fn runtime_tls_identity() -> anyhow::Result<(Vec<u8>, Vec<u8>)> {
    let mut guard = RUNTIME_TLS_IDENTITY
        .lock()
        .map_err(|_| anyhow!("Failed to lock runtime TLS identity store"))?;

    if let Some((cert_der, key_der)) = guard.as_ref() {
        return Ok((cert_der.clone(), key_der.clone()));
    }

    let cert = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .map_err(|e| anyhow!("Failed to generate runtime TLS certificate: {e}"))?;

    let cert_der = cert.cert.der().to_vec();
    let key_der = cert.signing_key.serialize_der();

    *guard = Some((cert_der.clone(), key_der.clone()));
    Ok((cert_der, key_der))
}

pub(crate) fn server_config() -> anyhow::Result<quinn::ServerConfig> {
    let (cert_der, key_der) = runtime_tls_identity()?;

    let cert = rustls::pki_types::CertificateDer::from(cert_der.clone());
    let key = rustls::pki_types::PrivateKeyDer::Pkcs8(rustls::pki_types::PrivatePkcs8KeyDer::from(
        key_der.clone(),
    ));

    let mut config = quinn::ServerConfig::with_single_cert(vec![cert], key)
        .context("Failed to create QUIC server config")?;
    config.transport = Arc::new(quinn::TransportConfig::default());
    Ok(config)
}
