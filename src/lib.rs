//! A loopback HTTP proxy that sends business data only on attested TLS sessions.
pub mod ca;
pub mod client;
pub mod config;
pub mod protocol;
pub mod proxy;
#[cfg(feature = "snp-verifier")]
pub mod snp;
#[cfg(feature = "tdx-verifier")]
pub mod tdx;

#[cfg(test)]
mod proxy_tests;
