<a id="客户端接入"></a>

# Client integration

[简体中文](CLIENTS.md) | **English**

<a id="本地入口"></a>

## Local endpoint

teeproxy runs as a local API gateway. Point the agent's API base URL at it; teeproxy establishes TLS to the configured Router, independently verifies its evidence and forwards requests. The default is `http://127.0.0.1:8899`. No local CA needs to be generated, installed or trusted. Do not use this address as HTTP_PROXY/HTTPS_PROXY.

Configure the Router address and approved policies as described in the root README. CLI mode obtains an enrolled identity's connection credential, restricted handshake JWT, signing seed and local API key through configured environment variables. Setting two keys alone does not replace identity enrollment. Run the Agent commands below in another terminal; the Agent needs only the local API key.

On Windows, register a new ID or select a local ID in **My account (我的账户)**. Configure the listener IP, port and API key in **Agent access (Agent 接入)**, select policies in **Connection and verification (连接与验证)**, then start the proxy. Overview copies the `/v1` URL used by Codex and Grok Build; Claude Code uses the origin without `/v1`.

| Client | Native protocol | teeproxy endpoint |
| --- | --- | --- |
| Codex | OpenAI Responses, HTTP/SSE | `POST /v1/responses` |
| Claude Code | Anthropic Messages, HTTP/SSE | `POST /v1/messages`, including `?beta=true` |
| Grok Build | xAI Responses, HTTP/SSE | `POST /v1/responses` |

All endpoints share one listening port; `/v1` is only a prefix. Clients select their protocol. teeproxy preserves JSON, tool arguments and streaming events, and passes client identity so Router can enforce Codex→ChatGPT, Claude Code→Claude and Grok Build→Grok matching.

## Codex

Merge the [provider example](../examples/codex.config.toml) into user configuration: `%USERPROFILE%/.codex/config.toml` on Windows or `~/.codex/config.toml` on Linux/macOS. `base_url` is `http://127.0.0.1:8899/v1`; `supports_websockets=false` selects the supported HTTP Responses/SSE transport.

PowerShell:

```powershell
$env:TEEPROXY_AGENT_KEY = 'YOUR_CONFIGURED_TEEPROXY_API_KEY'
codex --profile teeproxy --model YOUR_ROUTER_MODEL_ID
```

Linux/macOS:

```sh
export TEEPROXY_AGENT_KEY='YOUR_CONFIGURED_TEEPROXY_API_KEY'
codex --profile teeproxy --model YOUR_ROUTER_MODEL_ID
```

These are custom-provider fields documented in the [official configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference). Select a model ID from the Router catalog; configure the account pool through teeproxy's `pool` setting.

## Claude Code

Use the origin as the base URL. The SDK adds `/v1/messages`; do not add `/v1` twice.

PowerShell:

```powershell
$env:ANTHROPIC_BASE_URL = 'http://127.0.0.1:8899'
$env:ANTHROPIC_API_KEY = 'YOUR_CONFIGURED_TEEPROXY_API_KEY'
claude --model YOUR_ROUTER_MODEL_ID
```

Linux/macOS:

```sh
export ANTHROPIC_BASE_URL=http://127.0.0.1:8899
export ANTHROPIC_API_KEY='YOUR_CONFIGURED_TEEPROXY_API_KEY'
claude --model YOUR_ROUTER_MODEL_ID
```

Use API-key authentication. Existing `ANTHROPIC_AUTH_TOKEN` or authentication helpers must not override the teeproxy key. See the [official Claude Code gateway guide](https://code.claude.com/docs/en/llm-gateway). teeproxy accepts standard Bearer and `x-api-key` headers.

## Grok Build

Merge the [configuration example](../examples/grok.config.toml) into `%USERPROFILE%/.grok/config.toml` or `~/.grok/config.toml`. Use `base_url = "http://127.0.0.1:8899/v1"`, select `api_backend = "responses"`, and choose a catalog model under `grok/...`.

```powershell
$env:TEEPROXY_AGENT_KEY = 'YOUR_CONFIGURED_TEEPROXY_API_KEY'
grok -m teeproxy
```

Replace the example key with the configured teeproxy key. `supports_backend_search=false` reflects the currently unsupported upstream-hosted search tools; local tool calls still pass through Responses. See [Grok Build settings](https://docs.x.ai/build/settings).

<a id="vs-code-copilot-的自定义模型"></a>

## Custom models in VS Code Copilot

The local listener accepts valid Electron native-fetch requests, but this does not establish end-to-end support for arbitrary VS Code custom models. Router currently requires Codex, Claude Code or Grok Build identity matched to the provider's native interface. Local request-header compatibility is not general Copilot integration support.

The API key must match teeproxy. Version 0.12.2 fixed native Electron fetch carrying `Sec-Fetch-Site: none` being incorrectly rejected as `browser_request_rejected`; changing Copilot's networking components or disabling cross-site protections is unnecessary for that header issue.

<a id="agent-api-key-认证"></a>

## Agent API-key authentication

Every Agent endpoint checks the API key, including on loopback. CLI configuration in `teeproxy.toml`:

```toml
client_key_env = "TEEPROXY_CLIENT_KEY"
```

Supply a value of 16–256 visible ASCII characters through that environment variable. Configure the same value in the agent: `TEEPROXY_AGENT_KEY` for Codex or `ANTHROPIC_API_KEY` for Claude Code. This local key is separate from Router authentication and is not forwarded. It is mandatory on every listener. Supported headers are `Authorization: Bearer ...`, `x-api-key` or `x-goog-api-key`. If several authentication fields are supplied, all must match. Failure returns 401.

Desktop users enter the same format under **Agent access → API key**, without setting the teeproxy environment variable. Agents still use that value. Saved values load and display automatically after restart.

<a id="从-01-迁移"></a>

## Migrating from 0.1

Version 0.2 removed HTTP CONNECT, local CA and provider-domain mappings. Start from `teeproxy.example.toml`, retaining the Router address, key environment-variable names and approved-policy paths. Old `ca_dir`, `routes` and `proxy_token_env` settings are rejected. `client_key_env` replaced the former optional authentication mechanism; pool selection moved to top-level `pool`.

Remove HTTP_PROXY/HTTPS_PROXY values that previously pointed to teeproxy, and CODEX_CA_CERTIFICATE/NODE_EXTRA_CA_CERTS values used solely for the old teeproxy CA. Preserve unrelated proxy/CA settings as needed. If another HTTP proxy is in use, include `127.0.0.1,localhost,::1` in NO_PROXY. Old CA files are no longer read.

<a id="兼容范围"></a>

## Compatibility

The default listener is for the same machine. For another device, configure the host's LAN IP or `0.0.0.0` and a teeproxy API key. Remote agents use the actual host IP, such as `http://192.168.1.6:8899/v1`, not `0.0.0.0`. This leg remains HTTP and requires a trusted network. `https://127.0.0.1` would require a trusted server certificate; local HTTPS listening is not currently provided. teeproxy→Router always uses independently attested TLS 1.3.

Only `GET/HEAD /v1/models` and `POST /v1/responses`, `/v1/messages`, `/v1/chat/completions` are exposed. Unknown paths return 404 and wrong methods return 405 locally, without contacting Router. Supported generation requests and responses retain their native HTTP formats. teeproxy does not parse request bodies, tool calls, usage or refusals. Router handles account selection, Agent/provider matching, billing and refusal auditing.

HTTP/SSE is supported; WebSocket is not. Router does not yet implement auxiliary operations such as `/v1/responses/compact` or `/v1/messages/count_tokens`, which return 404 locally. New operations must be implemented in Router before joining the path/method allowlist; their body protocols need not be reimplemented in teeproxy.

`router_attestation_failed` means no business request was sent. `router_connection_failed_request_not_replayed` means failure during sending, with execution potentially unknown. teeproxy does not replay requests; the Agent itself may retry.

<a id="模型目录与接口边界"></a>

## Catalog and endpoint boundaries

`GET /v1/models` and HEAD use a platform catalog cache, without forwarding to Router. The desktop uses its authenticated platform session and current connection ID to query `/portal/api/inventory`; model, pool, account and Agent lists share this identity-scoped cache. Success is cached for five minutes, with refresh available on page entry. Temporary network failures retain old data with `X-Teeproxy-Catalog-Status: stale`. Explicit 401/403 clears the cache, stops the proxy and closes connections; page refresh and Agent queries share this behavior. Network errors, timeouts, 429 and 5xx do not stop it. After restoring authentication, restart the proxy manually. An initial load failure returns 503, never a direct Router fallback.

CLI mode reads the platform's publicly available catalog using `management_url`, default `https://teerouter.org`. Router always checks actual calling permissions; the catalog grants none. Model forwarding does not wait for catalog refresh.

Agent endpoints no longer include `/v1/pools`, `/v1/merchants`, `/v1/usage`, `/v1/identity`, `/v1/pricing`, `/health/live` or `/health/ready`, including subpaths. Internal attestation and routing negotiation may still read Router health. Desktop account, price, consumption and identity pages access the platform directly. The old `merchant-transport` command was removed; merchants use independent teemerchant.
