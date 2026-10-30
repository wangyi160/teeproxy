# teeproxy

[简体中文](README.md) | **English**

A local API endpoint for Codex, Claude Code and other agents. teeproxy independently verifies [teerouter](https://github.com/wangyi160/teerouter)'s hardware evidence, approved software release and TLS binding before sending business credentials or requests.

```text
Agent → http://127.0.0.1:8899/v1 → teeproxy → Verified TLS 1.3 → teerouter
```

Agents connect directly through their base URL. The default local endpoint is loopback HTTP, without a local certificate or HTTP-proxy setup. A specific local IP, `0.0.0.0` or `::` can also be configured; every listener requires a nonempty teeproxy API key. Connections from teeproxy to Router always use HTTPS with independent TEE verification.

<a id="windows-桌面版"></a>

## Windows desktop application

The Windows release is portable: extract it and run `teeproxy.exe` to open the native Win32 window. Configuration, sessions and TDX/SNP policies live in the adjacent `data` folder. In **My account (我的账户)**, register a new ID or select an existing local ID; the client automatically manages its Router identity. In **Agent access (Agent 接入)**, configure the local IP, port and API key. In **Connection and verification (连接与验证)**, select reviewed TDX/SNP policies. The API key authenticates Agent access locally and is not uploaded to the platform or Router.

- **Save settings:** writes `data/desktop.json`; external policy files are copied to `data/policies/` using relative paths. Keys remain encrypted with Windows DPAPI.
- **Verify connection:** verifies a real Router in the background and displays the result, without sending business credentials.
- **Start / stop proxy:** the upper-right button controls the local API listener. Stopping also closes existing connections and streams.
- **Overview:** shows the local `/v1` endpoint, upstream address and model/pool counts. Copy the endpoint for your agent's base URL.
- **Pools and models (账号池与模型):** three linked lists show pools above, accounts at lower left and models at lower right. Entering the page after authentication refreshes it; it also updates every five minutes while visible, without requiring the local proxy to run. Pools show merchant, multiplier, available-account count, provider and status. Disabled/unavailable accounts, pools without available accounts and models exclusive to those pools are hidden. Switching pools clears model search. Refresh preserves selection by pool ID; if the pool disappears, its details clear without automatically selecting another.
- **Apply a pool:** browsing does not change routing. **Use selected pool (使用所选账号池)** saves the selection; **Use this pool and copy model (使用此池并复制模型)** also copies its model ID. Both preserve the multiplier ceiling. Saving while running affects new requests immediately. Merchants set pool multipliers: 1× is the official base price and 0.2× is 20%. Catalogs come from platform snapshots; real model connections still require independent TEE verification.
- **Model names:** the UI and `/v1/models` use `chatgpt/model`, `claude/model` and `grok/model`, for example `grok/grok-4.7`. Prefixes identify providers. The same provider/model shares an ID across merchants. Router verifies the provider, selects a pool and sends the raw model name upstream.
- **Routing policy:** select a fixed pool or automatically match the agent's requested model; both modes require a multiplier ceiling. Save edits while running to affect new requests, while in-flight requests retain their original rules. Router performs selection and ceiling checks. Price priority includes estimated cache costs. Low-latency priority uses Router's five-minute probes through account proxies to official endpoints, with session affinity and no model speed-test requests. teeproxy does not fetch the catalog for every routing decision or rewrite the model/body; the UI displays Router's actual selection.
- **System tray:** minimizing or closing the window keeps forwarding active. Click the tray icon to restore it; choose **Exit teeproxy (退出 teeproxy)** to terminate.
- **My account:** register, select and use local IDs, inspect identity/connection status, or log out. No username/password or manual Router-key creation is required.
- **Top-ups and withdrawals (充值与提现):** displays balance, top-up forms and orders, loading on entry. A selected/new order opens a separate payment card with address QR, network, exact amount and expiry countdown; payment status refreshes automatically. Withdrawal mode accepts a USDT network, amount and destination, showing fees and net amount. User refunds use the same minimum and tiered fees as merchants; administrators process payouts manually. Pending applications can be cancelled.
- **Consumption details (消费明细):** shows charges and accounting status; select a record to inspect its signed receipt.

The platform registers identity public keys, holds balances/top-ups/receipts and issues restricted admission credentials. Model requests additionally require signatures from the local Ed25519 key. The local API key only authenticates the Agent. Listener configuration, API key, routing and trust policies remain local. Logout stops that identity's proxy and clears sessions/private page data, preserving local IDs and proxy settings. Users may access available pools from all enabled merchants without applying to each individually.

Windows packages are available under [Releases](https://github.com/wangyi160/teeproxy/releases). The sidebar price page reads baseline rates for currently available models from platform `/portal/api/pricing`, not Router. See [Desktop guide](docs/DESKTOP.en.md) for operation, storage and background behavior.

![Native Windows desktop overview](docs/images/windows-overview.png)

[Connection settings](docs/images/windows-desktop.png) · [Routing policy](docs/images/windows-routing.png). Screenshots use example addresses; after connecting and refreshing, the pools/models page shows real data accessible to the current identity.

<a id="源码构建和命令行"></a>

## Source builds and CLI

```sh
cargo build --release --locked
cp teeproxy.example.toml teeproxy.toml
```

Windows builds require the Windows SDK (MSVC) or a MinGW resource compiler. `cargo build --release --locked --all-features` enables both TDX and SNP. Running without arguments on Windows opens the desktop app; the CLI below remains available. Desktop users configure the window directly, without editing TOML or setting key environment variables.

Set the Router HTTPS origin and independently reviewed `tdx_policy` or `snp_policy` in `teeproxy.toml`. Relative paths resolve against the configuration file's directory. Example policies are empty and reject startup; see [Attestation and policies](docs/ATTESTATION.en.md).

CLI mode needs an enrolled identity's connection credential, restricted handshake JWT, local signing seed and Agent API key. Their environment-variable names are configured through `router_key_env`, `router_access_env`, `user_signing_seed_env` and `client_key_env`. The example signing-seed variable is `TEEPROXY_USER_SIGNING_SEED`; also configure `router_access_env` for the deployment. A seed is 32 bytes encoded as 64 hex characters and must match the enrolled public key; a newly generated random seed cannot replace an existing identity. Supply these through a protected environment, then run:

```sh
./target/release/teeproxy verify --config teeproxy.toml
./target/release/teeproxy run --config teeproxy.toml
```

`verify` neither reads the business Router key nor sends model requests. `run` listens on `127.0.0.1:8899` by default. Each new Router connection must pass verification. Verified connections can be reused for at most 120 seconds before re-verification; existing streams may finish.

Intel TDX/dstack verification is included by default. AMD SEV-SNP uses `cargo build --release --locked --features snp-verifier`, requiring a C toolchain and Perl for vendored OpenSSL.

<a id="连接-agent"></a>

## Connecting agents

| Client | API endpoint setting | Generation endpoint |
| --- | --- | --- |
| Codex | `base_url = "http://127.0.0.1:8899/v1"`, `wire_api = "responses"` | `POST /v1/responses` |
| Claude Code | `ANTHROPIC_BASE_URL=http://127.0.0.1:8899` | `POST /v1/messages` |
| Grok Build | `base_url = "http://127.0.0.1:8899/v1"`, `api_backend = "responses"` | `POST /v1/responses` |

`/v1` is a shared path prefix, not a declaration of Chat Completions. These clients retain their native formats; teeproxy does not convert them into `/v1/chat/completions`.

For Codex, merge the [provider example](examples/codex.config.toml) into user configuration, set `TEEPROXY_AGENT_KEY` to the API key configured in teeproxy, and run `codex --profile teeproxy --model YOUR_ROUTER_MODEL_ID`. The example uses HTTP Responses/SSE.

For Claude Code, set the base URL above and `ANTHROPIC_API_KEY` to the teeproxy API key, then run `claude --model YOUR_ROUTER_MODEL_ID`.

For Grok Build, merge the [model configuration](examples/grok.config.toml), set `TEEPROXY_AGENT_KEY`, and run `grok -m teeproxy`. `responses` preserves native tool calls and reasoning. Select an authorized `grok/...` ID from the catalog.

The Agent key must exactly match teeproxy's configured key; arbitrary placeholders do not work. It is validated locally and never forwarded to Router. teeproxy manages the actual Router connection credentials. See [Client integration](docs/CLIENTS.en.md) for complete PowerShell commands, authentication and migration.

<a id="行为和范围"></a>

## Behavior and scope

- Local endpoints are limited to `GET/HEAD /v1/models` and `POST /v1/responses`, `/v1/messages`, `/v1/chat/completions`. Unknown paths return 404; wrong methods return 405 locally, without opening a Router connection. Generation query strings, bodies and business headers pass through without parsing models, prompts, tools, usage or refusals.
- Hop-by-hop headers, Agent authentication and cookies are removed before setting Router authentication. Requests carry the saved fixed `pool` or automatic routing rules, with a default 1× ceiling. Caller-supplied pool/account/quote-version headers cannot override saved policy. The local catalog does not select pools.
- Router enforces pool and multiplier constraints before calling upstream. Response routing metadata is for display only; teeproxy does not independently revalidate or block it.
- Status and data stream through incrementally, including SSE. Cancellation, errors or timeouts close the corresponding upstream connection. Sent requests are never automatically replayed.
- Loopback is the default; specific or all-interface IP listeners still require the exact configured API key. Actual destination IP/port are checked. Cross-origin browser requests, CONNECT, proxy-form URLs, administrative endpoints and URL credentials are rejected.
- Router handles Agent/provider matching, account selection, multipliers, billing and refusal auditing. Errors from supported business endpoints pass through unchanged, without retries or stopping the proxy.
- Model bodies and decompressed gzip bodies are limited to 512 MiB, covering the published OpenAI 512 MB image-request range. Image data remains unchanged; upstream providers retain their own limits. Attestation and capability queries have separate limits. Defaults are 16 in-flight requests and 128 client connections. The first-response timeout is 300 seconds, configurable through `response_header_timeout_secs` (1–3600); stream-idle timeout is independently 120 seconds. Waiting too long returns HTTP 504 / `router_response_timeout_request_not_replayed` and logs the reason. Sent requests are not resent; Router and providers also have their own timeouts.

Transport currently supports HTTP/SSE, not WebSocket. Router does not implement `/v1/responses/compact`, `/v1/messages/count_tokens` and certain auxiliary operations; teeproxy returns 404 locally. Platform catalog queries returning 401/403 stop the proxy and close connections. Network errors, timeouts and 5xx do not. See [Security](docs/SECURITY.en.md) for deployment and local trust boundaries.

<a id="验证"></a>

## Verification

```sh
cargo test --locked
cargo test --locked --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

Tests cover direct local API access, authentication separation, same-TLS-channel use, unverified-evidence rejection, SSE, cancellation/timeouts, gzip, host/path restrictions and tampering/expiry/policy checks against public Intel/AMD signed samples. Mock successful verification exists only in `cfg(test)`; release binaries have no verification-bypass option. Real cloud instances and specific Agent versions still require integration acceptance.

The optional [CI template](ci/github-actions.yml) can be installed at `.github/workflows/ci.yml`; committing that path requires GitHub workflow write permission.

License: [MIT](LICENSE). See [NOTICE](NOTICE) for verification code and test-material sources.

Agent `/v1/models` returns only standard `id`, `object`, `created` and `owned_by` fields, deduplicated by model ID. Merchant, pool, account, multiplier, quote and cache metadata stay in the desktop UI. Generation responses remove internal `X-Router-*`, `X-Teeproxy-*`, cookies and server implementation details while preserving original bodies, SSE, usage, errors, request IDs and rate-limit/retry headers.

<a id="客户端持有签名身份019-起"></a>

<a id="client-owned-router-authentication-019"></a>

### Client-owned signing identities (since 0.19)

**Register new ID (注册新 ID)** generates an Ed25519 identity locally and registers its public key. No username or password is required. Any number of local IDs can be retained in portable `data/identities/`, encrypted with Windows DPAPI. Logout preserves keys. Login proves possession through a single-use, origin-bound challenge. Existing signing-identity migration preserves wallet and merchant history. Back up the data directory and preserve the original Windows user's decryption environment.

Management synchronizes public keys to Router. Model requests carry the registered ID, timestamp, nonce and signature without resending the public key. Each forwarded request has a fresh signature covering Router boot identity, user identity, HTTP method, exact path/query, timestamp, nonce, body digest and end-to-end headers, including routing rules. Router user IDs are `u_sig_` plus public-key SHA-256. A platform Bearer alone cannot impersonate the identity. The local Agent key remains separate and is not sent to Router.

Before business data is sent, attestation and health negotiation use a restricted login JWT. teeproxy requires Router signature protocol v2, without unsigned fallback. CLI users supply an enrolled local seed through `user_signing_seed_env`; SDK send methods also require a `Signer`.
