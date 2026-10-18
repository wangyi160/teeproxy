# 客户端接入

## 本地入口

teeproxy 作为本机 API 网关运行。把 agent 的 API base URL 指向它，teeproxy 负责向固定 router 建立 TLS、独立验证证明并转发请求。默认使用 `http://127.0.0.1:8899`，无需生成、安装或信任本地 CA。不要把这个地址设为 HTTP_PROXY/HTTPS_PROXY。

按根目录 README 填写 router 地址与批准策略，通过受保护的环境向 teeproxy 提供 `TEEPROXY_ROUTER_KEY` 后启动它。以下 agent 命令在另一个终端运行；agent 不需要真实 router key。

Windows 桌面版可直接在窗口填写上游地址、密钥和策略，并点击“启动网关”；无需为 teeproxy 配置环境变量。点击“复制地址”即可得到当前端口对应的 `/v1` 地址。

## Codex

将 [provider 示例](../examples/codex.config.toml) 合并到用户级配置（Windows `%USERPROFILE%/.codex/config.toml`，Linux/macOS `~/.codex/config.toml`）。其中 `base_url` 为 `http://127.0.0.1:8899/v1`，`supports_websockets=false` 选择 router 支持的 HTTP Responses/SSE。

PowerShell：

```powershell
$env:TEEPROXY_AGENT_KEY = 'teeproxy-local'
codex --profile teeproxy --model YOUR_ROUTER_MODEL_ID
```

Linux/macOS：

```sh
export TEEPROXY_AGENT_KEY=teeproxy-local
codex --profile teeproxy --model YOUR_ROUTER_MODEL_ID
```

这里使用的是 OpenAI Docs 中的自定义 provider 字段，参考 [官方配置文档](https://learn.chatgpt.com/docs/config-file/config-reference)。模型 ID 应来自 router 目录；账号池通过 teeproxy 的 `pool` 配置选择。

## Claude Code

Claude Code 的 base URL 使用根地址，SDK 自行追加 `/v1/messages`；不要重复添加 `/v1`。

PowerShell：

```powershell
$env:ANTHROPIC_BASE_URL = 'http://127.0.0.1:8899'
$env:ANTHROPIC_API_KEY = 'teeproxy-local'
claude --model YOUR_ROUTER_MODEL_ID
```

Linux/macOS：

```sh
export ANTHROPIC_BASE_URL=http://127.0.0.1:8899
export ANTHROPIC_API_KEY=teeproxy-local
claude --model YOUR_ROUTER_MODEL_ID
```

使用 API-key 模式。若现有环境还设置了 `ANTHROPIC_AUTH_TOKEN` 或其他认证助手，应避免它们覆盖本地 key；认证选择见 [Claude Code 官方网关说明](https://code.claude.com/docs/en/llm-gateway)。teeproxy 支持标准 Bearer 和 `x-api-key` 认证头。

## VS Code Copilot 的自定义模型

在 OpenAI 兼容提供商中将 API Base URL 设为 `http://127.0.0.1:8899/v1`，模型 ID 从 teeproxy 模型列表复制，并选择模型支持的接口。当前 Workbuddy 测试池仅支持 Chat Completions；Responses 和 Messages 需要各自兼容的官方账号池。

本地 key 未启用时可填 `teeproxy-local`；启用后须填写相同的本地 key。v0.12.2 修复了 Electron 原生 fetch 携带 `Sec-Fetch-Site: none` 时被误报为 `browser_request_rejected` 的问题，无需修改 Copilot 的网络组件或关闭跨站请求限制。

## 可选本地认证

默认不校验本地 API key，能访问监听地址的本机进程均可使用网关。agent 要求填写 key 时可以使用上述占位值。若需单独的本地访问密钥，在 `teeproxy.toml` 设置：

```toml
client_key_env = "TEEPROXY_CLIENT_KEY"
```

向 teeproxy 提供该环境变量，值为 16–256 个可见 ASCII 字符。将同一个值配置到 agent 的 API key：Codex 用 `TEEPROXY_AGENT_KEY`，Claude Code 用 `ANTHROPIC_API_KEY`。本地 key 与 router key 分离，仅用于 Agent 到 teeproxy，不转发 Router；非回环监听必须设置。支持 `Authorization: Bearer ...`、`x-api-key` 或 `x-goog-api-key`；请求含有多项认证时必须全部匹配。失败返回 401。

桌面版在“连接设置 → 本地密钥”直接填写同样格式的值，无需设置上述 teeproxy 环境变量；agent 的 API key 仍应与此值一致。

## 从 0.1 迁移

0.2 移除了 HTTP CONNECT、本地 CA 和供应商域名映射。重新使用 `teeproxy.example.toml`，保留自己的 router 地址、key 环境变量和批准策略路径。旧的 `ca_dir`、`routes`、`proxy_token_env` 配置会被拒绝；可选认证改用 `client_key_env`，账号池改为顶层 `pool`。

将此前指向 teeproxy 的 HTTP_PROXY/HTTPS_PROXY 设置撤销，并移除仅为旧 teeproxy 设置的 CODEX_CA_CERTIFICATE/NODE_EXTRA_CA_CERTS；其他代理或 CA 设置应按实际用途保留。若已有其他 HTTP 代理，确保 NO_PROXY 包含 `127.0.0.1,localhost,::1`。旧 CA 文件不再被程序读取。

## 兼容范围

默认入口供同一台机器使用。需要其他设备接入时，在“监听 IP”填写本机局域网 IP 或 `0.0.0.0`，并设置本地密钥；其他设备的 Base URL 使用这台电脑的实际 IP，例如 `http://192.168.1.6:8899/v1`，不能填写 `0.0.0.0`。这一段仍为 HTTP，应在可信网络中使用。`https://127.0.0.1` 仍然需要可信服务器证书，当前版本不提供本地 HTTPS 监听。teeproxy → router 始终使用独立 TEE 验证的 TLS 1.3。

请求保持原协议和模型名；不转换 OpenAI 与 Anthropic 格式。当前 router 未实现 `/v1/responses/compact`、`/v1/messages/count_tokens` 或 WebSocket，相关 agent 功能不可用。推理以外的登录、更新、遥测、MCP 等由 agent 自己的网络配置决定。

`router_attestation_failed` 表示没有发送业务请求；`router_connection_failed_request_not_replayed` 表示发送阶段失败，执行结果可能未知。teeproxy 不重放请求；agent 自身仍可能重试。
