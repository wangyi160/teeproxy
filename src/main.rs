use anyhow::{Context, Result, ensure};
use std::path::Path;
use teeproxy::{client, config::Config, proxy::Proxy};

const HELP: &str = "teeproxy 0.2.0

teeproxy verify --config teeproxy.toml
teeproxy run --config teeproxy.toml

Set your agent API base URL to http://127.0.0.1:8899/v1.
The gateway verifies the router before sending data over upstream TLS.
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
        "Expected a command and its --config argument; see --help"
    );
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
    let client_key = config
        .client_key_env
        .as_ref()
        .map(|name| {
            std::env::var(name)
                .map(zeroize::Zeroizing::new)
                .map_err(|_| anyhow::anyhow!("Required client key environment variable is unset"))
        })
        .transpose()?;
    let address = config.listen;
    let proxy = Proxy::new(config, trust, &key, client_key.as_ref().map(|v| v.as_str()))?;
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
