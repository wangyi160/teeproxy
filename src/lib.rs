//! A loopback API gateway that sends business data only on attested TLS sessions.
pub mod client;
pub mod config;
#[cfg(windows)]
pub mod desktop;
pub mod merchant_transport;
pub mod protocol;
pub mod proxy;
#[cfg(feature = "snp-verifier")]
pub mod snp;
#[cfg(feature = "tdx-verifier")]
pub mod tdx;

#[cfg(test)]
mod proxy_tests;
