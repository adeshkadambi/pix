//! TLS for the local Riot endpoints (League client on 127.0.0.1, later the Live Client API).
//!
//! Riot's root certificate `riotgames.pem` is the only trusted root for these clients. The chain is
//! always verified; only the hostname check is skipped, because Riot's local certificates aren't
//! always issued for 127.0.0.1 (SPEC §3). No `danger_accept_invalid_certs` anywhere.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{CryptoProvider, ring};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{
    CertificateError, ClientConfig, DigitallySignedStruct, Error, RootCertStore, SignatureScheme,
};

const RIOT_ROOT_PEM: &[u8] = include_bytes!("../certs/riotgames.pem");

/// Sets `ring` as the process-wide rustls provider. Call once at startup, before any HTTP client
/// is built; public HTTPS clients (Data Dragon, OP.GG) rely on it.
pub fn install_crypto_provider() {
    // Errors only if a provider is already installed, which is fine.
    let _ = ring::default_provider().install_default();
}

/// Verifies the chain against Riot's root and ignores a hostname mismatch.
#[derive(Debug)]
struct RiotVerifier {
    inner: Arc<WebPkiServerVerifier>,
}

impl ServerCertVerifier for RiotVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        intermediates: &[CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> Result<ServerCertVerified, Error> {
        match self.inner.verify_server_cert(end_entity, intermediates, server_name, ocsp_response, now)
        {
            // webpki checks the name only after the chain verified against Riot's root.
            Err(Error::InvalidCertificate(
                CertificateError::NotValidForName | CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            other => other,
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, Error> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

fn riot_roots() -> RootCertStore {
    let mut roots = RootCertStore::empty();
    for cert in CertificateDer::pem_slice_iter(RIOT_ROOT_PEM) {
        roots.add(cert.expect("bundled riotgames.pem is valid PEM")).expect("riotgames.pem is a valid trust anchor");
    }
    roots
}

/// The rustls config shared by every local Riot connection (HTTP and WebSocket).
pub fn riot_config() -> Arc<ClientConfig> {
    static CONFIG: OnceLock<Arc<ClientConfig>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let provider: Arc<CryptoProvider> = Arc::new(ring::default_provider());
            let inner = WebPkiServerVerifier::builder_with_provider(Arc::new(riot_roots()), provider.clone())
                .build()
                .expect("Riot root store builds a verifier");
            let config = ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .expect("ring supports the default TLS versions")
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(RiotVerifier { inner }))
                .with_no_client_auth();
            Arc::new(config)
        })
        .clone()
}

/// HTTP client for 127.0.0.1 Riot endpoints only. Trusts nothing but Riot's root.
pub fn local_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .tls_backend_preconfigured((*riot_config()).clone())
        .no_proxy()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .build()
        .expect("local Riot HTTP client builds")
}

/// HTTP client for public HTTPS (Data Dragon, OP.GG), using the system's trusted roots.
pub fn public_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!("Pix/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .expect("public HTTP client builds")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_riot_root_is_the_only_anchor() {
        assert_eq!(riot_roots().len(), 1);
    }

    #[test]
    fn clients_build() {
        install_crypto_provider();
        let _ = riot_config();
        let _ = local_http_client();
        let _ = public_http_client();
    }
}
