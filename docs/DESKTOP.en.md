<a id="windows-原生桌面版"></a>

# Native Windows desktop application

[简体中文](DESKTOP.md) | **English**

**My account (我的账户)** uses a local Ed25519 identity, without a username/password. **Register new ID (注册新 ID)** generates a key and registers its public key; **Local ID (本地 ID)** and **Use this ID (使用此 ID)** select an existing identity. Keys are encrypted under `data/identities/`, with session state separately in `data/identity.dpapi`. Startup restores and validates the session. Expired sessions reauthenticate using the local key; temporary outages preserve identity and encrypted data. Explicit logout ends the session without deleting local IDs.

`teeproxy.exe` uses native Win32 windows, text fields, buttons and file selectors, supporting Tab navigation and per-monitor DPI scaling. Double-click to open it; background threads handle attestation and API forwarding.

Windows x86_64 packages target Windows 10/11, run after extraction and require no administrator privileges. The two-column layout has navigation on the left and details on the right, with TDX/SNP verification, model/pool lists and a system tray.

<a id="左侧导航与右侧详情"></a>

## Navigation and detail pages

- **My account (我的账户):** register, select/use local IDs, inspect identity/connection status and log out.
- **Top-ups and withdrawals (充值与提现):** balance and top-up network/amount form above, historical orders below left and a payment card below right. The card displays an address QR code, network, exact amount and countdown, with copy actions. QR codes encode only the address and are generated locally; select the displayed network and enter the full amount when paying. Expired/paid orders hide the QR and disable copying payment information.
- **Consumption details (消费明细):** daily/monthly spend, request and pending-accounting counts, and per-request charges. Select a record and choose **Consumption receipt (消费凭据)** to view/copy its signed JSON.

Account features access the platform directly over HTTPS through native controls, without opening a browser. Entering a page loads current account data. Top-up orders refresh every 15 seconds and other pages every five minutes. A failed order creation retains its operation ID for explicit retry, without automatically repeating payment operations.

Users may access available pools of all enabled merchants without individual applications. The platform persists identities, balances, top-up orders, charges and receipts. The client maintains no local financial-record replica.

The Agent **API key** is generated, stored and checked locally, never uploaded to the platform or Router. The platform registers public keys and issues restricted admission credentials; model requests additionally require local private-key signatures. Identity/admission credentials are client-managed, not a second key to paste into an Agent. Local IP, port, routing settings and policies are not part of platform registration.

Session/connection state uses current-user Windows DPAPI in `data/identity.dpapi`; signing keys remain separately in `data/identities/`. Logout immediately stops the identity's proxy, clears catalog/private account content and ends the local session, while preserving signing identity, API key and settings. Switching local IDs requires fresh authentication; another identity's data cannot be reused.

- **Overview (概览):** proxy status, current identity and available balance above; Agent URL, copy action and access settings in the middle; routing mode, preference/fixed pool, ceiling and available pool/deduplicated model counts below. Links open logs, account, top-ups, routing and catalog pages. Entry loads account/catalog data with a loading indicator while retaining existing content.
- **Pools and models (账号池与模型):** three linked lists. Each upper pool row shows name, merchant, multiplier, available-account count, provider and status. Selected-pool accounts appear at lower left with status/catalog/model count; models/interfaces appear at lower right. Disabled/unavailable accounts and empty-availability pools are hidden, along with models exclusive to them. Search applies only to the selected pool and clears on pool switches. IDs use `chatgpt/model`, `claude/model` and `grok/model`; the same model shares an ID across merchant pools and is deduplicated in `/v1/models`.
- **Agent access (Agent 接入):** listener IP, port and fully displayed API key, with generate, enter, copy and save actions.
- **Connection and verification (连接与验证):** platform address, automatically bound Router address, TDX/SNP policy paths and browse buttons. Platform/Router addresses are locked while logged in and editable after logout.
- **Runtime log (运行记录):** verification, refresh, start, stop and error messages.
- **Routing policy (路由策略):** fixed-pool or automatic routing, current rules and last automatically chosen pool.
- **Model prices (模型价格):** baseline prices for models exposed by available pools. Models with no available accounts, disallowed or removed models are excluded; shared models are deduplicated. Filter ChatGPT/Claude/Grok or search. The table shows input/output/cache-read/context/tier information; details include cache writes, source, verification time and pricing revision. Units are USD per million tokens before the pool multiplier. Missing prices show “—”, never free.

Proxy status appears at lower left and start/stop at upper right. Local access settings and TEE verification have separate pages; catalog refresh is on the pools/models page.

Entering **Pools and models** after authentication refreshes automatically without requiring the proxy to run; it also refreshes every five minutes while visible. Authenticated platform `/portal/api/inventory` supplies an identity-scoped in-memory cache shared with local `GET /v1/models`. This does not contact Router or require prior TEE verification. Platform snapshots use the existing five-minute cycle. If identity binding or another operation is pending, loading resumes when ready; repeated clicks do not create concurrent fetches. Proxy startup also refreshes; **Refresh list (刷新列表)** remains available. Catalog 401/403 stops the proxy, closes connections and clears catalog/price lists; ordinary network failures do not. Background Agent catalog requests follow the same rule even while minimized.

Initial empty lists show centered loading indicators. Refresh preserves populated lists and animates the refresh button. The page avoids explanatory placeholders and duplicate pool summaries; errors use a brief status message.

Account, top-up, consumption and price pages share upper-right loading indicators, with centered indicators for empty tables. Refresh preserves content; completion, failure or cancellation stops the indicator. Login, receipt retrieval and order creation also show loading state.

The price page loads platform `GET /portal/api/pricing` on entry and through **Refresh prices (刷新价格)**. It reads platform cache without Router access or a running local proxy. Router checks its GitHub price source every five hours; the platform reuses differential checkpoints, updates unchanged-price caches every five hours and publishes changed pricing revisions without extra Router requests for this page. Receipts and balances synchronize every five minutes. Prices are filtered by the latest available-model catalog; transient concurrency exhaustion or latency-probe failure does not remove an otherwise available model. Router no longer exposes `/v1/pricing`.

Refresh failure preserves the previous successful list with a visible error. Changing Router, business credentials or approved policies clears old lists to prevent mixing connections. The UI does not create, delete or log in to upstream accounts; merchants manage those through teemerchant.

Browsing pools/accounts/models does not change routing. **Use selected pool (使用所选账号池)** saves it; **Use this pool and copy model (使用此池并复制模型)** also copies the complete model ID. The page remains open and the ceiling is preserved. Saving while running affects new requests; existing requests finish. `X-Router-Routing` carries saved rules; Router checks provider, model, pool and ceiling, then schedules the specific account.

All three lists remain visible and support mouse/keyboard pool selection. Refresh preserves the full pool ID despite reordering. If the pool disappears, account/model details clear until reselection. Unavailable accounts/pools disappear on refresh and reappear after recovery. Probe failure alone does not make an account unavailable.

<a id="配置上游"></a>

## Configuring the upstream

1. Register a new ID or use a local ID in **My account**. The client manages connection credentials and shows the binding once Router receives it; initial binding proceeds in the background without blocking the login page.
2. In **Connection and verification**, browse to an independently reviewed TDX/SNP policy and **Save (保存)**. Server-supplied release information is not automatically approved.
3. In **Agent access**, set IP, port and API key, then save. Supported listeners include `127.0.0.1`, a local IP, `0.0.0.0` and `::`; all require 16–256 visible ASCII key characters.
4. Select fixed/automatic routing and a multiplier ceiling in **Routing policy**.
5. **Start proxy (启动代理)** and configure the Agent with the overview URL and local key. Save errors preserve entered values and explain the failure.

For `0.0.0.0`, overview displays the locally usable `http://127.0.0.1:PORT/v1`; remote devices use the host's actual LAN IP, for example `http://192.168.1.6:8899/v1`. `0.0.0.0` is a bind address, not a remote destination. IPv6 uses brackets, such as `http://[::1]:8899/v1`. Network access remains HTTP and requires a trusted network; the host firewall also affects reachability.

`https://router.example.com:8443` is only a format example. Bundled empty policies approve no instance; initial access requires a trusted policy.

<a id="固定账号池与自动匹配"></a>

## Fixed pools and automatic routing

Refresh the catalog before opening **Routing policy**. **Fixed pool (指定账号池)** lists pool, merchant and multiplier. Mode, pool, ceiling and priority remain editable while running. **Save routing settings (保存路由设置)** atomically applies the new rules to subsequent requests. Unsaved edits do not affect running rules and are marked unsaved; failed saves preserve previous rules. Existing saved pools remain selected; browsing another pool does not switch routing.

Each incoming request captures one rule set for its entire lifetime. Saving does not interrupt streams or recheck old responses against a new ceiling. Before proxy startup, saved rules apply on the next start.

Both modes enforce the **multiplier ceiling**, default 1×, with at most six decimal places. A ceiling of 0.5 permits 0.5× but rejects higher multipliers before upstream calls. Pool changes preserve the ceiling. A fixed pool exceeding it returns HTTP 403 `routing_multiplier_exceeded`, without switching to another pool.

Automatic matching supports:

| Setting | Behavior |
| --- | --- |
| Multiplier ceiling | Maximum factor relative to the official base price, up to six decimals; 0.5 means at most 0.5× |
| Price priority | Compares estimated total costs, including cache reads/writes, among pools satisfying permissions, model, interface, availability and ceiling |
| Low-latency priority | Router probes official API links through account proxies every five minutes, preserves session accounts and compares connection latency; no model-token usage |

Pool rows show the lowest valid available-account link latency; account rows show their individual latency in ms and probe time in the local time zone. Unmeasured, failed, expired and unavailable states are distinct. Latency measures Router→proxy→official endpoint, not model generation speed. Session affinity may select an account other than the minimum. Visible pages refresh every five minutes or through **Refresh list**; expired measurements are labelled accordingly.

Latency includes proxy tunnel, TLS and HTTP response-header time. Router probes in the background; the client does not run speed tests. Without valid samples, routing falls back to availability, affinity and estimated cost. Older `speed` settings map to low-latency priority.

Models match the Agent's exact ID and interface, without separate category/subcategory settings or request rewriting. Automatic mode uses compatible pools authorized for the current identity. No match returns `routing_no_matching_pool`, without exceeding the ceiling or substituting another model. Obsolete model-selection fields from 0.10 are ignored and removed on the next save; ceilings remain.

Every request sends mode, optional fixed pool, ceiling and priority in `X-Router-Routing`. Router applies matching/admission within the TEE using one configuration/quote snapshot and archives the actual quote. It does not rely on client catalog snapshots or price-revision validation. The client does not fetch `/v1/models` or `/v1/pools` before generation to select a route; catalogs are for browsing.

Before the first business request on a verified TLS connection, routing-rules v1 is negotiated. Unsupported old Routers return `router_routing_protocol_required` before business data is sent. teeproxy overrides caller pool/account/quote-version/rule headers. Router enforces constraints before upstream calls; response pool/multiplier fields are display-only and are not independently compared or used to block replies. Internal `X-Router-*` and `X-Teeproxy-*` headers are not exposed to Agents. Requests are not replayed; Router status/body pass through. See the [routing protocol](https://github.com/wangyi160/teerouter/blob/main/docs/API.en.md#请求路由规则-v1).

Low-latency selection uses periodic HTTPS probes through account proxies, not model generation throughput. Link latency is neither time-to-first-token nor tokens/second. Missing samples fall back to availability, session affinity and estimated cost.

Both modes use public `/v1/` requests to carry rules, without requiring teeproxy to understand provider bodies. CLI fixed mode without `pool` returns `routing_pool_required` for generation; the catalog remains readable. Fewer than two available pools need no speed comparison, but this does not mean other pools have been measured.

<a id="运行行为"></a>

## Runtime behavior

- “Running” means the local API is listening; each upstream business connection still requires independent TEE/TLS-binding verification.
- **Verify connection** neither starts listening nor sends business Router credentials. Its result applies to that check; later connections verify again.
- Routing rules can be saved while running. Upstream address, API key, policies and listener IP/port remain locked connection settings: stop, save, then restart to change them.
- The upper-right action becomes **Stop proxy (停止代理)** while running or **Cancel operation (取消操作)** during standalone verification/refresh. It cancels background work, closes listeners/streams and returns to Start after completion.
- **Minimizing or closing the window hides it to the Windows tray and keeps forwarding.** Click the tray icon to restore, or use its menu to open/start/stop/exit. Only **Exit teeproxy (退出 teeproxy)** ends the process and releases the port.
- If tray registration fails, the window stays on the taskbar and closing exits, preventing an inaccessible hidden process. Explorer restarts trigger tray re-registration.
- The app does not automatically start listening, add startup entries, rewrite Agent configuration or install certificates. Open it and click Start when needed.
- The local API key loads and displays in full for inspection/copying; Agents must use that same value. Empty keys prevent save/start. Router credentials stay with teeproxy.

Codex uses `http://127.0.0.1:PORT/v1`; Claude Code's `ANTHROPIC_BASE_URL` uses `http://127.0.0.1:PORT`. See [Client integration](CLIENTS.en.md).

<a id="保存位置"></a>

## Storage locations

Portable Windows builds write to `data` beside `teeproxy.exe`, independently of the working directory:

```text
teeproxy.exe
data/
  desktop.json
  identity.dpapi
  identities/
    <identity-id>.dpapi
  device-id
  account-platform.json
  policies/
    phala-tdx.json
```

Saving copies externally selected policies unchanged into `data/policies/`, with relative paths in UI/configuration. Policies already inside `data` use paths relative to it. Same-name policies with different contents receive digest-suffixed filenames rather than overwrite existing files. Move the whole folder; upgrades replace the program while preserving `data`. An unwritable program directory produces an error instead of falling back to AppData.

On first upgrade without `data/desktop.json`, configuration, API key and policy copies migrate from `%LOCALAPPDATA%\teeproxy\desktop.json`. Existing portable configuration takes precedence. Old files remain as backups and are no longer read/updated. Migration failures report errors without overwriting old data with an empty configuration.

Addresses, ports, routing and policy paths use ordinary configuration. The local key is DPAPI-encrypted in `desktop.json`; sessions/connection credentials are in `identity.dpapi`; signing keys are in `identities/`. Files contain no plaintext credentials. Moving folders under the same Windows user preserves decryption; another user/machine usually cannot decrypt and requires credential recovery/reconfiguration. Corrupt or undecryptable files produce errors without silent replacement.

Save edited forms to persist them; unsaved values may be used for the current verification/run. Copying a policy does not change its approved releases, and full attestation still applies.

Use `teeproxy.exe gui --settings C:\path\desktop.json` for a separate desktop configuration. Desktop settings and CLI `teeproxy.toml` are independent; CLI credentials still come from environment variables.

TDX is enabled by default. SNP requires `--features snp-verifier` or `--all-features`; the UI indicates build support.

Before Windows releases, `tests/windows_desktop.ps1` checks native account pages, settings/identity separation, API-key persistence, rejection before login, verification failure and tray lifecycle. `-Portable` additionally checks default startup, external-policy copying, folder moves and no AppData writes. Tests use isolated local directories and test keys, not live Routers.

<a id="用户提现退款"></a>

### User withdrawals (refunds)

Switch **Top-ups and withdrawals** to **Withdraw (提现)** and enter network, amount and destination. Current platform fee tiers determine the displayed net amount before confirmation; users and merchants share minimums and rates. Submission freezes the full requested amount for manual administrator payout. Pending requests may be cancelled; cancellation/rejection unfreezes the full amount. Requests already being paid cannot be self-cancelled. Entry automatically loads balance, rules and request history.
