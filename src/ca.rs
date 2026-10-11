use anyhow::{Context, Result, ensure};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PublicKeyData,
};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, pem::PemObject};
use std::{
    fs::{DirBuilder, OpenOptions},
    io::Write,
    path::Path,
    sync::Arc,
};
use time::{Duration, OffsetDateTime};

/// Creates a new local CA. It never installs trust or replaces an existing CA.
pub fn init(dir: &Path) -> Result<()> {
    ensure!(
        !dir.join("ca.pem").exists() && !dir.join("ca.key").exists(),
        "CA already exists; refusing to replace it"
    );
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)?;
    let mut params = CertificateParams::new(Vec::<String>::new())?;
    params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    params
        .distinguished_name
        .push(DnType::CommonName, "teeproxy local CA");
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    params.not_before = OffsetDateTime::now_utc() - Duration::days(1);
    params.not_after = OffsetDateTime::now_utc() + Duration::days(365);
    let key = KeyPair::generate()?;
    let cert = params.self_signed(&key)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(dir.join("ca.key"))?
        .write_all(key.serialize_pem().as_bytes())?;
    options
        .open(dir.join("ca.pem"))?
        .write_all(cert.pem().as_bytes())?;
    Ok(())
}

pub struct LocalCa {
    issuer: Issuer<'static, KeyPair>,
    expires: OffsetDateTime,
}

impl LocalCa {
    pub fn load(dir: &Path) -> Result<Self> {
        let key_pem = zeroize::Zeroizing::new(
            std::fs::read_to_string(dir.join("ca.key"))
                .context("Run teeproxy init-ca before starting the proxy")?,
        );
        let key = KeyPair::from_pem(&key_pem)?;
        let pem = std::fs::read_to_string(dir.join("ca.pem"))?;
        let der = CertificateDer::from_pem_slice(pem.as_bytes())?;
        let (_, parsed) = x509_parser::parse_x509_certificate(&der)?;
        ensure!(
            parsed.is_ca() && parsed.validity().is_valid(),
            "Local CA is invalid or expired"
        );
        ensure!(
            parsed.public_key().raw == key.subject_public_key_info(),
            "Local CA certificate and key do not match"
        );
        let expires = OffsetDateTime::from_unix_timestamp(parsed.validity().not_after.timestamp())?;
        Ok(Self {
            issuer: Issuer::from_ca_cert_pem(&pem, key)?,
            expires,
        })
    }

    pub fn tls_for_host(&self, host: &str) -> Result<Arc<rustls::ServerConfig>> {
        let mut params = CertificateParams::new(vec![host.into()])?;
        params.distinguished_name.push(DnType::CommonName, host);
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.not_before = OffsetDateTime::now_utc() - Duration::minutes(5);
        params.not_after = self
            .expires
            .min(OffsetDateTime::now_utc() + Duration::days(30));
        let key = KeyPair::generate()?;
        let cert = params.signed_by(&key, &self.issuer)?;
        let mut config = rustls::ServerConfig::builder_with_provider(Arc::new(
            rustls::crypto::ring::default_provider(),
        ))
        .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
        .with_no_client_auth()
        .with_single_cert(
            vec![cert.der().clone()],
            PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
        )?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];
        Ok(Arc::new(config))
    }
}
