# teeproxy

**简体中文** | [English](README.en.md)


给 Codex、Claude Code 等 agent 提供一个本地 API 地址。teeproxy 独立验证 [teerouter](https://github.com/wangyi160/teerouter) 的硬件证明、批准的软件发布和 TLS 绑定，通过之后才向 router 发送密钥和业务请求。

```text
Agent → http://127.0.0.1:8899/v1 → teeproxy → 已验证的 TLS 1.3 → teerouter
```

agent 通过 base URL 直连 teeproxy。默认入口使用回环 HTTP，无需配置本地证书或 HTTP 代理。可配置本机具体 IP、`0.0.0.0` 或 `::`；所有监听地址都必须设置非空 teeproxy API key。teeproxy 到 router 的连接始终使用 HTTPS 和独立 TEE 校验。

<a id="windows-desktop-application"></a>

## Windows 桌面版

Windows 版为免安装便携程序，解压后双击 `teeproxy.exe` 打开原生 Win32 窗口。配置、登录状态和 TDX/SNP 策略保存在程序同目录的 `data` 文件夹。先在“我的账户”注册新 ID 或选择已有本地 ID，程序自动绑定当前账户的 Router 身份；在“Agent 接入”设置本地 IP、端口和 API key，在“连接与验证”选择审核后的 TDX/SNP 策略。API key 只供 Agent 访问本机代理，不上传平台或 Router。

- **保存设置**：将设置保存到 `data/desktop.json`，外部策略文件复制到 `data/policies/` 并保存相对路径；密钥仍由 Windows DPAPI 加密。
- **验证连接**：在后台校验真实 router，显示校验结果，不发送业务密钥。
- **启动代理 / 停止代理**：右上角按钮控制本地 API 监听；停止会同时关闭现有连接和响应流。
- **概览**：查看本地 `/v1` 地址、上游地址及模型和账号池数量，点击“复制地址”用于 agent 的 base URL。
- **账号池与模型**：同页展示三个联动列表：上方选择账号池，下方左侧显示池内账号，右侧显示池内模型。登录后每次进入页面自动刷新，页面可见时每 5 分钟更新，无需启动本地代理。池列表保留商户、倍率、账号数量、模型提供商和状态；账号数量只统计可用账号；停用、不可用账号及没有可用账号的池不展示，其专属模型也不进入列表。切换池会清除模型搜索条件；刷新按池 ID 保留选择，原池消失则清空详情，不自动换池。
- **使用账号池**：浏览池、账号和模型不会修改路由。“使用所选账号池”保存选定的池；“使用此池并复制模型”同时复制模型 ID。两者都保留倍率上限，运行中保存后新请求立即生效。倍率由商户按池设置，1× 表示官方基准价，0.2× 表示其 20%。目录由管理平台快照提供；真实模型连接仍须独立通过 TEE 验证。
- **模型名称**：界面和 `/v1/models` 使用 `chatgpt/模型名`、`claude/模型名`、`grok/模型名`，例如 `grok/grok-4.7`。前缀是模型提供商；同一提供商的同一模型跨商户共享 ID。Router 校验提供商并选择账号池后，将原始模型名交给上游。
- **路由策略**：指定一个账号池，或按 Agent 请求的模型自动匹配；两种模式均设置倍率上限。运行中可编辑并保存，保存后新请求立即采用新规则，正在进行的请求继续使用原规则。teeproxy 将规则随请求交给 router，由 router 统一选池、检查倍率。费率优先比较含缓存读写的预计费用；低延迟优先使用 Router 每 5 分钟通过账号代理探测官方入口的结果，并保留会话粘性，不发送模型测速请求。客户端不逐次拉取目录来选池，不修改模型或正文，界面显示 router 返回的实际选择。
- **系统托盘**：最小化或关闭窗口后继续转发；点击托盘图标恢复，右键选择“退出 teeproxy”结束程序。
- **我的账户**：注册新 ID、选择本地 ID、查看当前身份与连接状态、退出；不使用用户名密码，无需手工创建或应用 Router key。
- **充值与提现**：直接展示余额、充值表单和订单列表，进入后自动读取。选中或创建订单后，独立付款卡片展示收款二维码、网络、精确金额和有效期倒计时，付款状态自动刷新。切换到提现后，可选择 USDT 网络、填写金额与收款地址，实时查看手续费和到账金额；用户退款与商户共用最低门槛及阶梯费率，提交后由管理员人工处理，待处理申请可取消。
- **消费明细**：调用费用与核算状态，选中一条记录即可查看该条签名凭据。

平台负责身份公钥登记、余额、充值与消费凭据，并签发受限的接入凭据。用户请求还需本机 Ed25519 私钥签名；本地 API key 只认证 Agent。监听地址、API key、路由规则和信任策略保存在本机。退出会停止当前身份的代理、清空会话与私有页面数据，保留本地 ID 和代理设置。用户可使用所有启用商户的可用账号池，无需逐家申请。

Windows 发行包见 [Releases](https://github.com/wangyi160/teeproxy/releases)。桌面侧栏的模型价格页通过管理平台 `/portal/api/pricing` 查看当前可用模型的基准价，不直接查询 Router。具体操作、配置保存位置和后台行为见 [桌面版说明](docs/DESKTOP.md)。

![Windows 原生桌面概览](docs/images/windows-overview.png)

[连接设置](docs/images/windows-desktop.png) · [路由策略](docs/images/windows-routing.png)。截图使用示例地址；连接并刷新后，“账号池与模型”页显示当前身份可访问的真实数据。

<a id="source-builds-and-cli"></a>

## 源码构建和命令行

```sh
cargo build --release --locked
cp teeproxy.example.toml teeproxy.toml
```

Windows 构建需要 Windows SDK（MSVC）或 MinGW 资源编译器。`cargo build --release --locked --all-features` 同时启用 TDX 和 SNP。Windows 无参数运行打开桌面窗口；以下命令行接口继续可用。桌面版直接在窗口里配置，无需编辑 TOML 或设置密钥环境变量。

在 `teeproxy.toml` 填写 router 的 HTTPS 根地址和自己审核的 `tdx_policy` 或 `snp_policy`。相对路径以配置文件所在目录为基准。示例策略为空，启动会拒绝；策略建立方法见 [证明与策略](docs/ATTESTATION.md)。

命令行模式需要已登记身份的连接凭据、受限握手 JWT、本地签名种子和 Agent API key。`router_key_env`、`router_access_env`、`user_signing_seed_env`、`client_key_env` 分别指定其环境变量名；示例中的签名种子变量为 `TEEPROXY_USER_SIGNING_SEED`，还需按部署填写 `router_access_env`。签名种子为 32 字节、64 个十六进制字符，必须对应已登记公钥，不能用随机新种子替代已有身份。通过受保护环境提供这些值，再执行：

```sh
./target/release/teeproxy verify --config teeproxy.toml
./target/release/teeproxy run --config teeproxy.toml
```

`verify` 不读取 router key，也不发送业务请求。`run` 默认监听 `127.0.0.1:8899`。每个新建 router 连接都必须通过验证；已验证连接最多在 120 秒内复用，过期重新验证，正在进行的流可继续完成。

默认包含 Intel TDX/dstack 验证。AMD SEV-SNP 使用 `cargo build --release --locked --features snp-verifier`，还需要编译 vendored OpenSSL 所需的 C 工具链和 Perl。

<a id="connecting-agents"></a>

## 连接 agent

| 客户端 | API 地址设置 | 生成接口 |
| --- | --- | --- |
| Codex | `base_url = "http://127.0.0.1:8899/v1"`，`wire_api = "responses"` | `POST /v1/responses` |
| Claude Code | `ANTHROPIC_BASE_URL=http://127.0.0.1:8899` | `POST /v1/messages` |
| Grok Build | `base_url = "http://127.0.0.1:8899/v1"`，`api_backend = "responses"` | `POST /v1/responses` |

`/v1` 是公共路径前缀，不代表 Chat Completions。上述三种客户端直接使用各自原生格式，teeproxy 不将它们转换成 `/v1/chat/completions`。

Codex：将 [provider 示例](examples/codex.config.toml) 合并到用户配置，设置 `TEEPROXY_AGENT_KEY=<teeproxy中配置的API-key>`，再运行 `codex --profile teeproxy --model <router模型ID>`。示例使用 HTTP Responses/SSE。

Claude Code：设置上述 `ANTHROPIC_BASE_URL` 和 `ANTHROPIC_API_KEY=<teeproxy中配置的API-key>`，运行 `claude --model <router模型ID>`。

Grok Build：将 [模型配置示例](examples/grok.config.toml) 合并到用户配置，设置 `TEEPROXY_AGENT_KEY`，运行 `grok -m teeproxy`。使用 `responses` 保留原生工具调用与推理内容；模型 ID 必须是目录内获准调用的 `grok/...`。

Agent 必须填写与 teeproxy 配置完全一致的 API key，不能使用任意占位值。这个 API key 仅用于本机校验，不转发 Router；真实 Router 连接凭据由 teeproxy 管理。完整 PowerShell 命令、认证和版本迁移见 [客户端接入](docs/CLIENTS.md)。

<a id="behavior-and-scope"></a>

## 行为和范围

- 入口仅开放 `GET/HEAD /v1/models` 和 `POST /v1/responses`、`POST /v1/messages`、`POST /v1/chat/completions`。未知路径在本地返回 404，已知路径使用错误方法返回 405，不建立 Router 连接。生成请求的查询参数、正文和业务头仍直接透传，不解析模型、prompt、tools、usage 或拒答字段。
- 清理逐跳头、agent 认证和 Cookie，设置 router key；业务请求携带固定 `pool` 或自动匹配规则，默认倍率上限为 1×。调用方的池、账号及报价版本头不能覆盖已保存的策略。本地模型目录不选池。
- 账号池和倍率约束由 Router 在调用上游前执行；响应中的路由信息仅用于展示，不在 teeproxy 中再次校验或拦截。
- 响应状态和数据逐块转发，支持 SSE；取消、错误或超时关闭对应上游连接。已发出的请求不会自动重放。
- 默认监听本机回环地址；支持配置 IP 和所有网卡监听，所有地址必须使用与配置一致的 teeproxy API key。核对请求实际连接的本机 IP 和端口，拒绝浏览器跨域请求、CONNECT、代理形式的目标 URL、管理接口和 URL 凭证。
- Router 负责 agent 与模型匹配、账号选择、倍率约束、计费和拒答审计；支持的接口返回业务错误时，teeproxy 原样转发，不重试也不因该错误停止代理。
- 模型请求体及 gzip 解压后的上限均为 512 MiB，覆盖公开 OpenAI 图片请求的 512 MB 范围；图片内容保持原样，上游仍检查自身的模型与请求限制。TEE 证据与能力查询继续使用各自的独立上限。默认 16 个并发请求和 128 个客户端连接。业务请求等待首个响应默认 300 秒，命令行配置项为 `response_header_timeout_secs`（1–3600 秒）；流空闲超时独立计算，默认 120 秒。等待响应超时返回 HTTP 504 / `router_response_timeout_request_not_replayed`，运行记录显示原因；已发送的请求不会自动重发。Router 和模型上游仍有各自的超时限制。

当前传输支持 HTTP/SSE，尚不支持 WebSocket。Router 尚未实现 `/v1/responses/compact`、`/v1/messages/count_tokens` 等辅助操作，teeproxy 在本地返回 404。管理平台目录查询返回 401/403 时停止代理并关闭现有连接；超时、网络故障及 5xx 不触发停机。部署和本机信任边界见 [安全说明](docs/SECURITY.md)。

<a id="verification"></a>

## 验证

```sh
cargo test --locked
cargo test --locked --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

测试涵盖本地 API 直连、认证隔离、同一 TLS 通道、拒绝未验证证据、SSE、取消与超时、gzip、主机和路径限制，以及公开 Intel/AMD 签名样本的篡改、过期和策略校验。成功通道测试的模拟验证入口仅存在于 `cfg(test)`，发行二进制没有跳过验证的选项。真实云实例和具体 agent 版本仍需接入验收。

可选 [CI 模板](ci/github-actions.yml) 可放入 `.github/workflows/ci.yml`；提交该路径需要 GitHub workflow 写权限。

许可证：[MIT](LICENSE)。验证代码和测试材料来源见 [NOTICE](NOTICE)。

Agent 的 `/v1/models` 仅返回标准字段 `id`、`object`、`created`、`owned_by`，相同模型 ID 去重。商户、账号池、账号、倍率、报价和缓存状态保留在客户端界面，不发给 Agent。生成响应移除 `X-Router-*`、`X-Teeproxy-*`、Cookie 和服务端实现信息，保留原始正文、SSE、usage、错误状态、请求 ID 和限流重试头。


<a id="client-owned-signing-identities-since-019"></a>

<a id="client-owned-router-authentication-019"></a>

### 客户端持有签名身份（0.19 起）

点击“注册新 ID”会在本机生成 Ed25519 身份并登记公钥，不需要用户名密码。身份选择器支持多个本地 ID，密钥保存在便携目录 `data/identities/` 并由 Windows DPAPI 加密。退出保留密钥；登录通过一次性、绑定来源的挑战证明持有私钥。已有签名身份迁移保持钱包与商户历史。请备份数据目录，并保留原 Windows 用户的解密环境。

管理平台向 Router 同步公钥。模型请求携带已登记 ID、时间戳、随机数和签名，不重复发送公钥。每个转发请求均使用新签名，覆盖 Router 启动标识、用户身份、HTTP 方法、精确路径/查询、时间、随机数、正文摘要以及包含路由规则的端到端请求头。Router 用户 ID 为 `u_sig_` 加公钥 SHA-256。平台 Bearer 本身不能冒充该身份发起模型请求；本地 Agent API key 是独立凭据，不发给 Router。

发送业务数据前，TEE 证明与健康协商使用受限的登录 JWT。teeproxy 要求 Router 签名协议 v2，没有无签名兼容回退。CLI 通过 `user_signing_seed_env` 提供已登记的本地签名种子；SDK 发送接口同样要求 `Signer`。
