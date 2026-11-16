use std::sync::Arc;

use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, RootCertStore, SignatureScheme};
use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};

/// 只用于显式开启「允许无效证书」的场景。
///
/// PVE 自签证书是内网自建的常态，所以这个能力必须有；但它会让中间人
/// 可以截获 `PVEAuthCookie`。因此由设置控制，且连接时会记录指纹用于变化告警。
#[derive(Debug)]
struct AcceptAnyServerCert(Arc<rustls::crypto::CryptoProvider>);

impl ServerCertVerifier for AcceptAnyServerCert {
    fn verify_server_cert(
        &self,
        _end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        Ok(ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.0.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.0
            .signature_verification_algorithms
            .supported_schemes()
    }
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    rustls::crypto::ring::default_provider().into()
}

/// 给 tokio-tungstenite 用的上游 WS 客户端 TLS 配置。
pub fn ws_client_config(allow_invalid_certs: bool) -> AppResult<ClientConfig> {
    let provider = provider();

    let builder = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| AppError::Tls(e.to_string()))?;

    let config = if allow_invalid_certs {
        builder
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(AcceptAnyServerCert(provider)))
            .with_no_client_auth()
    } else {
        let mut roots = RootCertStore::empty();
        roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        builder
            .with_root_certificates(roots)
            .with_no_client_auth()
    };

    Ok(config)
}

/// 计算证书指纹（SHA-256，冒号分隔大写十六进制），用于 TOFU 变化告警。
pub fn fingerprint(der: &[u8]) -> String {
    let digest = Sha256::digest(der);
    digest
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":")
}

/// 抓取服务器 TLS 证书的 DER 字节，用于计算指纹。
///
/// PVE 常见自签证书，因此这里不做信任判断 —— 只取叶子证书用于比对，
/// 真正的信任决策由 `allow_invalid_certs` 控制。
pub async fn fetch_server_cert_fingerprint(host: &str, port: u16) -> Option<String> {
    use std::sync::Mutex;

    use tokio_rustls::TlsConnector;

    #[derive(Debug)]
    struct Capture {
        slot: Arc<Mutex<Option<Vec<u8>>>>,
        provider: Arc<rustls::crypto::CryptoProvider>,
    }

    impl ServerCertVerifier for Capture {
        fn verify_server_cert(
            &self,
            end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &ServerName<'_>,
            _ocsp: &[u8],
            _now: UnixTime,
        ) -> Result<ServerCertVerified, rustls::Error> {
            if let Ok(mut slot) = self.slot.lock() {
                *slot = Some(end_entity.as_ref().to_vec());
            }
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls12_signature(
                message,
                cert,
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }

        fn verify_tls13_signature(
            &self,
            message: &[u8],
            cert: &CertificateDer<'_>,
            dss: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, rustls::Error> {
            rustls::crypto::verify_tls13_signature(
                message,
                cert,
                dss,
                &self.provider.signature_verification_algorithms,
            )
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            self.provider
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    let slot = Arc::new(Mutex::new(None));
    let provider = provider();

    let config = ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .ok()?
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(Capture {
            slot: slot.clone(),
            provider,
        }))
        .with_no_client_auth();

    let connector = TlsConnector::from(Arc::new(config));
    let stream = tokio::net::TcpStream::connect((host, port)).await.ok()?;
    let server_name = ServerName::try_from(host.to_string()).ok()?;

    // 握手可能因对端不是 TLS 而失败，但证书通常已在此前送达，取到就返回
    let _ = connector.connect(server_name, stream).await;

    let captured = slot.lock().ok().and_then(|s| s.clone());
    captured.map(|der| fingerprint(&der))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fingerprint_is_colon_separated_uppercase() {
        let fp = fingerprint(b"hello");
        assert_eq!(fp.len(), 32 * 3 - 1);
        assert!(fp.chars().all(|c| c.is_ascii_hexdigit() || c == ':'));
        assert_eq!(fp, fp.to_uppercase());
    }
}

