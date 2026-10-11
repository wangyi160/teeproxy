# 客户端接入

## 先启动代理

按根目录 README 创建 CA，填写 router 地址与本地批准的策略，并向 **teeproxy 进程**注入真实的 `TEEPROXY_ROUTER_KEY`。这个 key 是 router 的普通业务 key，不是 router 管理凭证。启动代理后，再在单独终端配置 agent。

teeproxy 是用户本机的可信进程，会接触提示词和回答。请将 CA 私钥保存在自己的用户私有目录；不要共享 `ca.key`。Unix 创建的目录/文件权限分别为 0700/0600；Windows 使用目录继承的 ACL，应放在当前用户独占的目录下。`init-ca` 不覆盖现有 CA，也不安装系统信任。

## PowerShell

假定当前目录是 teeproxy 的配置目录：

```powershell
$env:HTTP_PROXY = 'http://127.0.0.1:8899'
$env:HTTPS_PROXY = $env:HTTP_PROXY
$env:NO_PROXY = 'localhost,127.0.0.1,::1'
$env:CODEX_CA_CERTIFICATE = (Resolve-Path '.teeproxy/ca/ca.pem').Path
$env:NODE_EXTRA_CA_CERTS = $env:CODEX_CA_CERTIFICATE
```

这只改变当前终端及它启动的进程。Windows 环境变量名称不区分大小写。Linux/macOS 如果已有小写代理变量，应一起更新，避免覆盖大写设置。

Codex：将 [配置示例](../examples/codex.config.toml) 中的 provider/profile 合并到自己的配置，接着：

```powershell
$env:TEEPROXY_AGENT_KEY = 'teeproxy-local'
codex --profile teeproxy --model YOUR_ROUTER_MODEL_ID
```

使用独立 provider 将 `supports_websockets` 设置为 false，以匹配 router 的 HTTP Responses/SSE 接口。选择 router 目录中的真实模型 ID，不要将账号池前缀拼到模型名中；账号池可在 teeproxy 的对应 route 上配置 `pool`。

Codex 的 CA 环境变量与 provider 字段参考 [OpenAI 身份验证文档](https://learn.chatgpt.com/docs/auth) 和 [配置参考](https://learn.chatgpt.com/docs/config-file/config-reference)。

Claude Code：

```powershell
$env:ANTHROPIC_API_KEY = 'teeproxy-local'
claude --model YOUR_ROUTER_MODEL_ID
```

使用 API-key 模式接入 router；占位 key 只满足 agent 本地设置，teeproxy 会替换认证。无需向 agent 提供供应商账号或 router 管理密钥。Claude Code 的代理和 CA 设置参考 [官方网络配置](https://code.claude.com/docs/en/network-config)。终端、后台 agent 和 Desktop 的环境继承规则不同；这里的命令针对从该终端启动的 CLI。

## 可选代理认证

如需限制本机其他进程使用代理，配置：

```toml
proxy_token_env = "TEEPROXY_CLIENT_TOKEN"
```

向 teeproxy 提供该环境变量，长度至少 16 字符。agent 的代理 URL 使用 `http://teeproxy:PERCENT_ENCODED_TOKEN@127.0.0.1:8899`。这个 token 与 router key 分离，仅用于本地代理认证；不要提交到配置、脚本或 Git。省略该设置时，能访问监听地址的本机进程均可使用代理。

## 兼容限制

- HTTPS CONNECT 只接受配置的精确域名和 443 端口；TLS 内的 Host 必须与 CONNECT 一致。HTTP absolute-form 请求同样核对 URI 与 Host。
- 本地 TLS 支持 TLS 1.2/1.3，ALPN 为 HTTP/1.1；router 通道强制 TLS 1.3。没有 HTTP/2 或 WebSocket 桥接。
- 未配置的域名与供应商登录、遥测等非 router API 会失败。CLI 的自动更新、远程工具下载或 MCP 服务需要独立配置其网络，不会被代理偷偷直连。
- router 尚不提供 `/v1/responses/compact` 和 `/v1/messages/count_tokens`。依赖这些接口的 agent 功能不可用；不要把基本推理协议测试视为所有 agent 功能兼容。
- router key 不会提供给 agent。teeproxy 删除标准供应商认证、Cookie 和代理认证头，使用自身配置的 router key；额外自定义头仍可能被转发，不要在其内放置其他服务的密钥。
- teeproxy 不自动重放已发送请求；agent 自身仍可能有重试策略，按其配置控制。
- 本项目不改变 agent 的模型选择、思考级别、prompt、tools 或 tool_choice。router 的权限与协议检查仍然生效。

可用 `teeproxy verify --config teeproxy.toml` 单独排查取证。错误 `router_attestation_failed` 表示没有发送业务请求；`router_connection_failed_request_not_replayed` 表示发送阶段失败，执行结果可能未知。流中断后不追加伪造的完成事件。
