use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub listen: SocketAddr,
    pub router: String,
    pub ca_dir: PathBuf,
    pub router_key_env: String,
    pub tdx_policy: Option<PathBuf>,
    pub snp_policy: Option<PathBuf>,
    pub proxy_token_env: Option<String>,
    #[serde(default = "concurrency")]
    pub max_in_flight: usize,
    #[serde(default = "connections")]
    pub max_connections: usize,
    #[serde(default = "idle_seconds")]
    pub stream_idle_secs: u64,
    pub routes: Vec<Route>,
}

fn concurrency() -> usize {
    16
}
fn connections() -> usize {
    128
}
fn idle_seconds() -> u64 {
    120
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub host: String,
    #[serde(default = "v1")]
    pub prefix: String,
    #[serde(default = "v1")]
    pub router_prefix: String,
    pub pool: Option<String>,
}
fn v1() -> String {
    "/v1".into()
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = std::fs::read(path).context("Cannot read proxy configuration")?;
        ensure!(bytes.len() <= 64 * 1024, "Configuration too large");
        let mut config: Self = toml::from_str(std::str::from_utf8(&bytes)?)
            .map_err(|_| anyhow::anyhow!("Invalid proxy configuration"))?;
        let parent = path.parent().unwrap_or(Path::new("."));
        config.ca_dir = parent.join(&config.ca_dir);
        config.tdx_policy = config.tdx_policy.map(|p| parent.join(p));
        config.snp_policy = config.snp_policy.map(|p| parent.join(p));
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.listen.ip().is_loopback(),
            "The proxy must listen on a loopback IP"
        );
        let url = reqwest::Url::parse(&self.router).context("Invalid router URL")?;
        ensure!(
            url.scheme() == "https"
                && url.host_str().is_some()
                && url.username().is_empty()
                && url.password().is_none()
                && url.path() == "/"
                && url.query().is_none()
                && url.fragment().is_none(),
            "Router must be an HTTPS origin without credentials or path"
        );
        ensure!(
            (1..=128).contains(&self.max_in_flight)
                && (1..=1024).contains(&self.max_connections)
                && (1..=3600).contains(&self.stream_idle_secs),
            "Invalid resource limits"
        );
        ensure!(
            !self.router_key_env.is_empty(),
            "router_key_env is required"
        );
        ensure!(
            (1..=32).contains(&self.routes.len()),
            "Configure 1 to 32 exact host routes"
        );
        let mut hosts = BTreeSet::new();
        for route in &self.routes {
            ensure!(
                valid_host(&route.host) && hosts.insert(&route.host),
                "Invalid or duplicate route host"
            );
            for prefix in [&route.prefix, &route.router_prefix] {
                ensure!(
                    prefix.starts_with('/')
                        && !prefix.ends_with('/')
                        && !prefix.contains("..")
                        && prefix
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"/-_".contains(&c)),
                    "Invalid path prefix"
                );
            }
            ensure!(
                matches!(route.router_prefix.as_str(), "/v1" | "/v1beta"),
                "Unsupported router prefix"
            );
            if let Some(pool) = &route.pool {
                ensure!(
                    !pool.is_empty()
                        && pool.len() <= 128
                        && pool
                            .bytes()
                            .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)),
                    "Invalid pool"
                );
            }
        }
        Ok(())
    }

    pub fn trust(&self) -> Result<crate::client::Trust> {
        let mut trust = crate::client::Trust::default();
        let mut releases = 0;
        if let Some(path) = &self.tdx_policy {
            #[cfg(feature = "tdx-verifier")]
            {
                let verifier = crate::tdx::TdxVerifier::new(crate::tdx::Policy::load(path)?)?;
                releases += verifier.approved_release_count();
                trust.tdx = Some(verifier);
            }
            #[cfg(not(feature = "tdx-verifier"))]
            {
                let _ = path;
                anyhow::bail!("Build with tdx-verifier for a TDX policy");
            }
        }
        if let Some(path) = &self.snp_policy {
            #[cfg(feature = "snp-verifier")]
            {
                let verifier = crate::snp::SnpVerifier::new(crate::snp::Policy::load(path)?)?;
                releases += verifier.approved_release_count();
                trust.snp = Some(verifier);
            }
            #[cfg(not(feature = "snp-verifier"))]
            {
                let _ = path;
                anyhow::bail!("Build with snp-verifier for an SNP policy");
            }
        }
        ensure!(
            releases > 0,
            "No locally approved router releases; supply an independently reviewed policy"
        );
        Ok(trust)
    }
}

fn valid_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && host.contains('.')
        && host.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part
                    .bytes()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
}
