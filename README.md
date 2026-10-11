# teeproxy

让 Codex、Claude Code 等客户端通过本地 HTTP 代理访问 [teerouter](https://github.com/wangyi160/teerouter)。teeproxy 独立校验 router 的硬件证明、客户端批准的发布策略、新随机挑战与当前 TLS 证书绑定；通过之后才发送 router key 和业务请求。

```text
Agent ── HTTP proxy / HTTPS CONNECT ──> teeproxy（用户本机）
                                         │ 独立 TEE 验证
                                         └── 同一条已验证 TLS 连接 ──> teerouter
```

这是应用层透明代理：客户端保留供应商 API 的请求格式，配置的域名被映射到固定 router；模型、消息、工具参数不由代理改写。它支持 HTTP/1.1、HTTPS CONNECT 和 SSE，含本地 CA、连接复用、流取消、并发上限及超时。业务 POST 发送后失败不会被代理自动重发。

**HTTPS 接入除了设置代理地址，还需要 agent 信任本地 CA。** CA 只用于 agent → teeproxy；teeproxy → router 使用独立 TEE 验证，不信任本地 CA 或系统 CA 对 router 身份的判断。teeproxy 不自动安装系统根证书。

## 构建

```sh
cargo build --release --locked
./target/release/teeproxy --help
```

默认包含 Intel TDX/dstack 验证。AMD SEV-SNP 可选：

```sh
cargo build --release --locked --features snp-verifier
```

SNP 构建还需要编译 vendored OpenSSL 所需的 C 工具链与 Perl。项目不依赖管理平台、浏览器前端或另一个本地源码仓库。

## 初始化和运行

```sh
cp teeproxy.example.toml teeproxy.toml
./target/release/teeproxy init-ca --dir .teeproxy/ca
```

在 `teeproxy.toml` 中设置真实 router HTTPS 地址、自己审核的 `tdx_policy` 或 `snp_policy` 文件，以及需要转发的域名和可选账号池。相对路径以配置文件所在目录为基准。示例策略为空，启动时会拒绝；它不是对任何云实例的默认认可。策略格式与审核要求见 [证明与策略](docs/ATTESTATION.md)。

通过受保护的启动环境提供 `TEEPROXY_ROUTER_KEY`，然后执行：

```sh
./target/release/teeproxy verify --config teeproxy.toml
./target/release/teeproxy run --config teeproxy.toml
```

`verify` 独立检查实时连接，不读取 router key，也不发送业务请求。`run` 默认监听 `127.0.0.1:8899`，每个新建 router 连接都必须完成验证；已验证连接最多在 120 秒窗口内复用。过期会重新验证；正在进行的流可以继续完成。

## 接入 agent

在启动 agent 的终端设置以下变量。不要把这些代理变量设到 teeproxy 自己的启动环境；router 连接和证书材料下载使用独立出站连接。

```sh
export HTTP_PROXY=http://127.0.0.1:8899
export HTTPS_PROXY=http://127.0.0.1:8899
export http_proxy="$HTTP_PROXY"
export https_proxy="$HTTPS_PROXY"
export NO_PROXY=localhost,127.0.0.1,::1
export no_proxy="$NO_PROXY"
export CODEX_CA_CERTIFICATE="$PWD/.teeproxy/ca/ca.pem"
export NODE_EXTRA_CA_CERTS="$PWD/.teeproxy/ca/ca.pem"
```

`HTTPS_PROXY` 的值仍为 **http://**：这是本地 HTTP 代理地址，目标 HTTPS 请求通过 CONNECT 进入。供应商域名不能被 `NO_PROXY` 排除。

- **Codex**：合并 [provider 示例](examples/codex.config.toml)，设置 `TEEPROXY_AGENT_KEY=teeproxy-local`，再运行 `codex --profile teeproxy --model <router目录中的模型ID>`。示例显式使用 HTTP Responses，关闭 WebSocket。
- **Claude Code**：在此终端设置 `ANTHROPIC_API_KEY=teeproxy-local`，运行 `claude --model <router目录中的模型ID>`。供应商凭证由 router 管理，agent 的占位 key 不会转发到 router。
- **其他客户端**：配置相同 HTTP 代理和本地 CA，使用配置中的域名及 router 支持的 API。

Windows PowerShell 的完整命令、可选代理认证与兼容范围见 [客户端接入](docs/CLIENTS.md)。

## 转发范围

| 入口 | 转发到 router |
| --- | --- |
| `api.openai.com/v1/*` | `/v1/*` |
| `api.anthropic.com/v1/*` | `/v1/*` |
| `chatgpt.com/backend-api/codex/*` | `/v1/*` |
| `generativelanguage.googleapis.com/v1beta/*` | `/v1beta/*` |
| `api.x.ai/v1/*` | `/v1/*` |

这些是精确域名与路径前缀映射，不会直接连接相应供应商。实际允许的是 router 已实现的模型/账号池/用量查询、Chat Completions、Responses、Messages 和 Gemini 生成接口。管理 API、未知域名、URL 中的凭证和 WebSocket 被拒绝；没有绕过验证的直连回退。

代理保留请求方法、查询参数、原始 JSON 字节及兼容头；删除逐跳头、供应商认证与 Cookie，再设置 router 认证和配置的可选池。gzip 请求在 2 MiB 解压上限内解压，JSON 不经解析或重写。响应状态与数据逐块转发，强制 `Cache-Control: no-store`。

## 测试与边界

```sh
cargo test --locked
cargo test --locked --all-features
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
```

测试覆盖 CONNECT/本地证书、认证隔离、同一 TLS 通道、拒绝未验证证据、SSE、取消和并发、gzip、路径限制，以及公开 Intel/AMD 签名样本的篡改、过期、证书链和发布策略验证。成功通道测试使用仅在 `cfg(test)` 存在的模拟验证入口；发行二进制没有 sample、跳过验证或接受未知发布的选项。

测试不等于已在你的云实例或每一版 agent 上完成实机验收。当前不支持 Responses WebSocket、远程 compaction、Claude token-counting 接口、OAuth 登录转发或任意互联网代理。模型名与请求功能需要 router 本身支持；不会合成不支持接口的成功响应。详情见 [安全边界](docs/SECURITY.md)。

许可证：[MIT](LICENSE)。校验代码与测试材料来源见 [NOTICE](NOTICE) 和各 fixtures 目录。
