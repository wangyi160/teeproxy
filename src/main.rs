use anyhow::{Context, Result, ensure};
use std::path::Path;
use teeproxy::{ca, client, config::Config, proxy::Proxy};

const HELP: &str = "teeproxy 0.1.0

teeproxy init-ca --dir .teeproxy/ca
teeproxy verify --config teeproxy.toml
teeproxy run --config teeproxy.toml

The proxy listens on loopback and verifies the router before sending data.
Agent HTTPS traffic requires trust in ca.pem. No system trust is installed.
The router key is read only from the environment named by router_key_env.
";

#[tokio::main]
async fn main() {
    if let Err(error) = execute().await {
        eprintln!("teeproxy: {error:#}");
        std::process::exit(1);
    }
}

async fn execute() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] || args == ["-h"] {
        print!("{HELP}");
        return Ok(());
    }
    if args == ["--version"] {
        println!("teeproxy {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    ensure!(
        args.len() == 3,
        "Expected a command and its --config/--dir argument; see --help"
    );
    if args[0] == "init-ca" && args[1] == "--dir" {
        ca::init(Path::new(&args[2]))?;
        println!(
            "Local CA created at {}. Trust ca.pem only in your agent process.",
            args[2]
        );
        return Ok(());
    }
    ensure!(
        matches!(args[0].as_str(), "run" | "verify") && args[1] == "--config",
        "Unknown command; see --help"
    );
    let config = Config::load(Path::new(&args[2]))?;
    let trust = config.trust()?;
    if args[0] == "verify" {
        let attempt = client::verify(&config.router, &trust).await;
        println!("{}", serde_json::to_string_pretty(&attempt.report)?);
        ensure!(attempt.report.ok, "Router attestation failed");
        return Ok(());
    }
    let key = zeroize::Zeroizing::new(
        std::env::var(&config.router_key_env)
            .map_err(|_| anyhow::anyhow!("Required router key environment variable is unset"))?,
    );
    let proxy_token = config
        .proxy_token_env
        .as_ref()
        .map(|name| {
            std::env::var(name)
                .map(zeroize::Zeroizing::new)
                .map_err(|_| anyhow::anyhow!("Required proxy token environment variable is unset"))
        })
        .transpose()?;
    let address = config.listen;
    let proxy = Proxy::new(
        config,
        trust,
        &key,
        proxy_token.as_ref().map(|v| v.as_str()),
    )?;
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .context("Cannot bind local proxy")?;
    eprintln!(
        "teeproxy listening on http://{}; attestation required for every upstream session",
        listener.local_addr()?
    );
    tokio::select! {
        result = proxy.serve(listener) => result?,
        result = tokio::signal::ctrl_c() => result?,
    }
    Ok(())
}
