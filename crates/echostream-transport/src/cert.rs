//! TLS 证书工具：自签名证书生成与客户端验签配置

use std::sync::Arc;

use echostream_proto::Error;
use rustls_pki_types::{CertificateDer, PrivateKeyDer};

/// 生成自签名证书（开发环境开箱即用）
pub fn self_signed() -> echostream_proto::Result<(Vec<CertificateDer<'static>>, PrivateKeyDer<'static>)> {
    let certified_key = rcgen::generate_simple_self_signed(vec!["localhost".to_string()])
        .map_err(|e| Error::Io(format!("生成自签名证书失败: {e}")))?;
    let cert = certified_key.cert.der().clone();
    let key = PrivateKeyDer::Pkcs8(certified_key.key_pair.serialize_der().into());
    Ok((vec![cert], key))
}

/// 开发环境的客户端 TLS 配置：跳过证书验证（0-RTT 等能力不受影响）
pub fn insecure_client_config() -> echostream_proto::Result<rustls::ClientConfig> {
    use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use rustls::{DigitallySignedStruct, SignatureScheme};

    #[derive(Debug)]
    struct NoVerify;

    impl ServerCertVerifier for NoVerify {
        fn verify_server_cert(
            &self,
            _end_entity: &CertificateDer<'_>,
            _intermediates: &[CertificateDer<'_>],
            _server_name: &rustls_pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls_pki_types::UnixTime,
        ) -> std::result::Result<ServerCertVerified, rustls::Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &CertificateDer<'_>,
            _dss: &DigitallySignedStruct,
        ) -> std::result::Result<HandshakeSignatureValid, rustls::Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    Ok(rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(NoVerify))
        .with_no_client_auth())
}
