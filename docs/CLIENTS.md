# 客户端接入

## 本地入口

teeproxy 作为本机 API 网关运行。把 agent 的 API base URL 指向它，teeproxy 负责向固定 router 建立 TLS、独立验证证明并转发请求。默认使用 `http://127.0.0.1:8899`，无需生成、安装或信任本地 CA。不要把这个地址设为 HTTP_PROXY/HTTPS_PROXY。

按根目录 README 填写 router 地址与批准策略，通过受保护的环境向 teeproxy 提供 `TEEPROXY_ROUTER_KEY` 和 `TEEPROXY_CLIENT_KEY` 后启动它。以下 agent 命令在另一个终端运行；agent 不需要真实 router key。

Windows 桌面版在“我的账户”登录后自动绑定 Router，在“Agent 接入”配置监听 IP、端口和 API key，在“连接与验证”选择信任策略，然后点击“启动代理”。概览复制的是 Codex 和 Grok Build 使用的 `/v1` 地址；Claude Code 填写不带 `/v1` 的根地址。

| 客户端 | 原生协议 | teeproxy 入口 |
| --- | --- | --- |
| Codex | OpenAI Responses，HTTP/SSE | `POST /v1/responses` |
| Claude Code | Anthropic Messages，HTTP/SSE | `POST /v1/messages`，包括 `?beta=true` |
| Grok Build | xAI Responses，HTTP/SSE | `POST /v1/responses` |

这些接口同时开放在同一个监听端口，`/v1` 只是路径前缀。客户端自行选择协议；teeproxy 原样保留 JSON、工具参数和流式事件，并传递客户端标识供 Router 执行 Codex→ChatGPT、Claude Code→Claude、Grok Build→Grok 的匹配检查。

## Codex

将 [provider 示例](../examples/codex.config.toml) 合并到用户级配置（Windows `%USERPROFILE%/.codex/config.toml`，Linux/macOS `~/.codex/config.toml`）。其中 `base_url` 为 `http://127.0.0.1:8899/v1`，`supports_websockets=false` 选择 router 支持的 HTTP Responses/SSE。

PowerShell：

```powershell
$env:TEEPROXY_AGENT_KEY = 'YOUR_CONFIGURED_TEEPROXY_API_KEY'
codex --profile teeproxy --model YOUR_ROUTER_MODEL_ID
```

Linux/macOS：

```sh
export TEEPROXY_AGENT_KEY='YOUR_CONFIGURED_TEEPROXY_API_KEY'
codex --profile teeproxy --model YOUR_ROUTER_MODEL_ID
```

这里使用的是 OpenAI Docs 中的自定义 provider 字段，参考 [官方配置文档](https://learn.chatgpt.com/docs/config-file/config-reference)。模型 ID 应来自 router 目录；账号池通过 teeproxy 的 `pool` 配置选择。

## Claude Code

Claude Code 的 base URL 使用根地址，SDK 自行追加 `/v1/messages`；不要重复添加 `/v1`。

PowerShell：

```powershell
$env:ANTHROPIC_BASE_URL = 'http://127.0.0.1:8899'
$env:ANTHROPIC_API_KEY = 'YOUR_CONFIGURED_TEEPROXY_API_KEY'
claude --model YOUR_ROUTER_MODEL_ID
```

Linux/macOS：

```sh
export ANTHROPIC_BASE_URL=http://127.0.0.1:8899
export ANTHROPIC_API_KEY='YOUR_CONFIGURED_TEEPROXY_API_KEY'
claude --model YOUR_ROUTER_MODEL_ID
```

使用 API-key 模式。若现有环境还设置了 `ANTHROPIC_AUTH_TOKEN` 或其他认证助手，应避免它们覆盖 teeproxy API key；认证选择见 [Claude Code 官方网关说明](https://code.claude.com/docs/en/llm-gateway)。teeproxy 支持标准 Bearer 和 `x-api-key` 认证头。

## Grok Build

将 [配置示例](../examples/grok.config.toml) 合并到 `%USERPROFILE%/.grok/config.toml` 或 `~/.grok/config.toml`：`base_url` 使用 `http://127.0.0.1:8899/v1`，`api_backend` 必须选 `responses`，模型选 Router 目录内的 `grok/...`。

```powershell
$env:TEEPROXY_AGENT_KEY = 'YOUR_CONFIGURED_TEEPROXY_API_KEY'
grok -m teeproxy
```

将示例值换成 teeproxy 中配置的 API key。`supports_backend_search=false` 对应本项目暂不支持上游托管搜索工具的范围；本地工具调用仍通过 Responses 传递。配置字段见 [Grok Build 官方说明](https://docs.x.ai/build/settings)。

## VS Code Copilot 的自定义模型

在 OpenAI 兼容提供商中将 API Base URL 设为 `http://127.0.0.1:8899/v1`，模型 ID 从 teeproxy 模型列表复制，并选择模型支持的接口。当前 Workbuddy 测试池仅支持 Chat Completions；Responses 和 Messages 需要各自兼容的官方账号池。

API key 必须填写 teeproxy 中配置的同一个值。v0.12.2 修复了 Electron 原生 fetch 携带 `Sec-Fetch-Site: none` 时被误报为 `browser_request_rejected` 的问题，无需修改 Copilot 的网络组件或关闭跨站请求限制。

## Agent API key 认证

所有 Agent 接口均校验 API key，回环地址也没有例外。命令行在 `teeproxy.toml` 设置：

```toml
client_key_env = "TEEPROXY_CLIENT_KEY"
```

向 teeproxy 提供该环境变量，值为 16–256 个可见 ASCII 字符。将同一个值配置到 agent 的 API key：Codex 用 `TEEPROXY_AGENT_KEY`，Claude Code 用 `ANTHROPIC_API_KEY`。teeproxy API key 与 router key 分离，仅用于 Agent 到 teeproxy，不转发 Router；任何监听地址都必须设置。支持 `Authorization: Bearer ...`、`x-api-key` 或 `x-goog-api-key`；请求含有多项认证时必须全部匹配。失败返回 401。

桌面版在“Agent 接入 → API key”直接填写同样格式的值，无需设置上述 teeproxy 环境变量；agent 的 API key 仍应与此值一致。保存后重启会自动显示已配置的值，无需重新填写。

## 从 0.1 迁移

0.2 移除了 HTTP CONNECT、本地 CA 和供应商域名映射。重新使用 `teeproxy.example.toml`，保留自己的 router 地址、key 环境变量和批准策略路径。旧的 `ca_dir`、`routes`、`proxy_token_env` 配置会被拒绝；可选认证改用 `client_key_env`，账号池改为顶层 `pool`。

将此前指向 teeproxy 的 HTTP_PROXY/HTTPS_PROXY 设置撤销，并移除仅为旧 teeproxy 设置的 CODEX_CA_CERTIFICATE/NODE_EXTRA_CA_CERTS；其他代理或 CA 设置应按实际用途保留。若已有其他 HTTP 代理，确保 NO_PROXY 包含 `127.0.0.1,localhost,::1`。旧 CA 文件不再被程序读取。

## 兼容范围

默认入口供同一台机器使用。需要其他设备接入时，在“监听 IP”填写本机局域网 IP 或 `0.0.0.0`，并设置 teeproxy API key；其他设备的 Base URL 使用这台电脑的实际 IP，例如 `http://192.168.1.6:8899/v1`，不能填写 `0.0.0.0`。这一段仍为 HTTP，应在可信网络中使用。`https://127.0.0.1` 仍然需要可信服务器证书，当前版本不提供本地 HTTPS 监听。teeproxy → router 始终使用独立 TEE 验证的 TLS 1.3。

teeproxy 仅开放 `GET/HEAD /v1/models` 和 `POST /v1/responses`、`POST /v1/messages`、`POST /v1/chat/completions`。未知路径返回 404，已知路径使用错误方法返回 405，均在本地处理，不联系 Router。支持的生成接口保持原生 HTTP 请求和响应，不解析请求正文、工具调用、usage 或拒答内容。账号选择、agent 与模型匹配、计费及拒答审计均由 Router 执行。

当前传输支持 HTTP/SSE，尚不支持 WebSocket。Router 尚未实现 `/v1/responses/compact`、`/v1/messages/count_tokens` 等辅助操作，这些路径在本地返回 404。新增操作须在 Router 实现后加入路径与方法清单；无需在 teeproxy 中实现其正文协议。

`router_attestation_failed` 表示没有发送业务请求；`router_connection_failed_request_not_replayed` 表示发送阶段失败，执行结果可能未知。teeproxy 不重放请求；agent 自身仍可能重试。

## 模型目录与接口边界

`GET /v1/models`（以及 HEAD）由 teeproxy 从管理平台目录缓存返回，不转发 Router。桌面版使用登录会话和当前连接 ID 请求 `/portal/api/inventory`，模型、账号池及账号三个列表与 Agent 模型列表共用同一缓存。成功结果缓存 5 分钟；页面进入时可以刷新。网络暂时不可用时保留旧目录，响应头 `X-Teeproxy-Catalog-Status: stale` 标记缓存；目录查询明确返回 401/403 时清空缓存、停止代理并关闭现有连接；页面刷新和 Agent 读取目录使用同一处理逻辑。网络故障、超时、429 和 5xx 不触发停机。重新登录或修复授权后需手动启动代理。首次加载失败返回 503，绝不回退查询 Router。

命令行模式使用配置项 `management_url`（默认 `https://teerouter.org`）读取管理平台公开可用目录。实际调用权限始终由 Router 检查，目录不授予权限。模型请求转发不等待管理平台目录刷新。

Agent 入口不再开放 `/v1/pools`、`/v1/merchants`、`/v1/usage`、`/v1/identity`、`/v1/pricing`、`/health/live`、`/health/ready`，各自子路径也被拦截。内部 TEE 握手和路由协议检查仍可读取 Router 健康信息。账号、价格、消费与身份查询由桌面界面直接访问管理平台。旧 `merchant-transport` 命令已删除；商户使用独立 teemerchant。
