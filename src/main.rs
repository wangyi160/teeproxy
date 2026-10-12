#![cfg_attr(windows, windows_subsystem = "windows")]
use anyhow::{Context, Result, ensure};
use std::path::Path;
use teeproxy::{client, config::Config, proxy::Proxy};

const HELP: &str = "teeproxy 0.5.0

teeproxy                         Windows native GUI
teeproxy gui [--settings PATH]  Windows native GUI with a separate profile

teeproxy verify --config teeproxy.toml
teeproxy run --config teeproxy.toml

Set your agent API base URL to http://127.0.0.1:8899/v1.
The gateway verifies the router before sending data over upstream TLS.
The router key is read only from the environment named by router_key_env.
";

fn main() {
    #[cfg(windows)]
    {
        let args: Vec<_> = std::env::args().skip(1).collect();
        let gui = args.is_empty() || args.first().is_some_and(|v| v == "gui");
        if gui {
            let result = (|| -> Result<()> {
                let path = match args.as_slice() {
                    [] | [_] => None,
                    [_, option, path] if option == "--settings" => Some(path.into()),
                    _ => anyhow::bail!("Usage: teeproxy gui [--settings PATH]"),
                };
                teeproxy::desktop::run(path)
            })();
            if let Err(error) = result {
                let text: Vec<u16> = format!("{error:#}").encode_utf16().chain(Some(0)).collect();
                unsafe {
                    windows_sys::Win32::UI::WindowsAndMessaging::MessageBoxW(
                        std::ptr::null_mut(),
                        text.as_ptr(),
                        windows_sys::core::w!("teeproxy"),
                        windows_sys::Win32::UI::WindowsAndMessaging::MB_OK
                            | windows_sys::Win32::UI::WindowsAndMessaging::MB_ICONERROR,
                    );
                }
                std::process::exit(1);
            }
            return;
        }
        // Preserve redirected CLI handles; attach only when launched from a console.
        unsafe {
            use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, System::Console::*};
            let output = GetStdHandle(STD_OUTPUT_HANDLE);
            if output.is_null() || output == INVALID_HANDLE_VALUE {
                AttachConsole(ATTACH_PARENT_PROCESS);
            }
        }
    }
    let result = tokio::runtime::Runtime::new()
        .context("Cannot start runtime")
        .and_then(|runtime| runtime.block_on(execute()));
    if let Err(error) = result {
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
