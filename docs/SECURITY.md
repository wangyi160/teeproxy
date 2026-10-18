# 信任与运行边界

teeproxy、agent、它们所在的用户主机及批准策略都在信任边界内。默认通过回环 HTTP 接入，也可配置本机具体 IP、`0.0.0.0` 或 `::`。非回环监听必须配置 teeproxy API key；提示词、回答和 teeproxy API key 在 Agent 到 teeproxy 这一段不使用 TLS，因此网络接入需要可信网络或受保护的通道。旧版桌面配置未指定 IP 时继续使用 `127.0.0.1`。

teeproxy 不生成本地 CA，不拦截供应商 TLS。它只连接配置中固定的 router HTTPS 地址。新建上游连接必须通过硬件证明、经审核的软件/启动策略及当前 TLS 通道绑定；成功之前不发送 router key 或业务数据。证书材料获取失败、未知策略、过期、调试或无签名证据均拒绝，没有未经验证的回退。

默认监听 127.0.0.1:8899；最多 128 个客户端连接、16 个并发请求，每个请求体及 gzip 解压后不超过 2 MiB。请求读取超时 30 秒，router 响应头超时 90 秒，流空闲默认 120 秒。请求名额保持到流完成、失败或取消。连接只在响应完整读取后复用，取消或错误使该连接关闭，已发送的请求不会由网关重放。

请求 Host 必须匹配实际接入的本机 IP 和端口，`localhost` 仅对回环目标有效；URI 只接受 origin-form 的允许 API 路径。CONNECT、绝对目标 URL、浏览器请求和管理 API 都会被拒绝。带 Origin（含 `null`）的请求始终拒绝；VS Code Copilot 等 Electron 原生客户端可携带无 Origin 的 `Sec-Fetch-Site: none`、`Sec-Fetch-Dest: empty` 和 API fetch 模式（`no-cors` / `cors` / `same-origin`）。这三个头须齐全且各只有一个值，网页来源、导航请求及歧义头仍拒绝，不开放浏览器 CORS。这些本机 Fetch Metadata 头不会转发上游。teeproxy API key 校验和 Host 限制照常执行。teeproxy 不按传入 Host 选择上游，不允许客户端改变上游目标。

`client_key_env` 在回环监听时可选，非回环监听时必填；未提供或提供错误的 teeproxy API key 时返回 401，不连接 Router。请求 Host 必须匹配接受连接的实际本机 IP 和端口；`localhost` 仅对回环目标有效，通配监听不会放宽 Host 检查。teeproxy 清理 agent 认证、Cookie 和逐跳头，再设置固定 router key。额外自定义头仍可能转发，请勿在其中放其他服务密钥。程序不会记录请求体、回答或认证值，响应强制 Cache-Control: no-store。

Windows 桌面版的密钥由 DPAPI 以当前用户范围加密保存，运行时在本机内存中使用。连接设置中的 Agent API key 自动加载并明文显示，Router 业务密钥仍需勾选后显示。运行记录不包含业务内容或密钥。停止操作等待监听及已有客户端连接关闭后才允许重启。最小化或关闭窗口会隐藏到系统托盘并继续转发；在托盘菜单选择“退出 teeproxy”才会结束后台运行。托盘不可用时，关闭窗口会退出程序。

router 及云平台仍须提供匹配批准策略的启动链。TEE 证明不代表供应商实际执行了某个模型，也不等于上游零日志。项目自带空批准策略；测试与历史签名样本不构成对任何实际部署的审批或生产安全审计。
