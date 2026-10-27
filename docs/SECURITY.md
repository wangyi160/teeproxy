# 信任与运行边界

teeproxy、agent、它们所在的用户主机及批准策略都在信任边界内。默认通过回环 HTTP 接入，也可配置本机具体 IP、`0.0.0.0` 或 `::`。所有监听地址都必须配置非空 teeproxy API key；提示词、回答和 teeproxy API key 在 Agent 到 teeproxy 这一段不使用 TLS，因此网络接入需要可信网络或受保护的通道。旧版桌面配置未指定 IP 时继续使用 `127.0.0.1`。

teeproxy 不生成本地 CA，不拦截供应商 TLS。它只连接配置中固定的 router HTTPS 地址。新建上游连接必须通过硬件证明、经审核的软件/启动策略及当前 TLS 通道绑定；成功之前不发送 router key 或业务数据。证书材料获取失败、未知策略、过期、调试或无签名证据均拒绝，没有未经验证的回退。

默认监听 127.0.0.1:8899；最多 128 个客户端连接、16 个并发请求，每个请求体及 gzip 解压后不超过 2 MiB。请求读取超时 30 秒，业务请求等待 router 响应头默认 300 秒（可配置），Router 自身的只读查询为 90 秒，流空闲默认 120 秒。请求名额保持到流完成、失败或取消。连接只在响应完整读取后复用，取消或错误使该连接关闭，已发送的请求不会由网关重放。

Agent 入口只接受 `GET/HEAD /v1/models` 及 `POST /v1/responses`、`POST /v1/messages`、`POST /v1/chat/completions`。目录来自管理平台缓存；仅三个生成入口转发 Router。未知路径返回 404，错误方法返回 405，均不建立上游连接。健康查询仅用于内部 TEE 握手后的协议协商。teeproxy 不解析供应商正文；agent 与模型匹配、账号与价格约束、用量和拒答判断由已验证的 Router 执行。业务请求始终携带已保存的路由规则，由 Router 在调用上游前执行限制。响应路由元数据只用于展示，不参与放行判断；缺失、异常或与请求规则不同均不会使 teeproxy 丢弃回复。

管理平台目录查询返回 401/403 时撤销该目录缓存并停止代理：关闭监听，结束现有客户端流。页面刷新和本地 `/v1/models` 共用这一机制，最小化窗口不影响执行；旧账户缓存的延迟失败不会作用于新账户。网络故障、超时、429、5xx 或格式错误不触发停机，模型业务接口返回的拒绝也不触发此停机逻辑。

请求 Host 必须匹配实际接入的本机 IP 和端口，`localhost` 仅对回环目标有效；URI 只接受 origin-form 的允许 API 路径。CONNECT、绝对目标 URL、浏览器请求和管理 API 都会被拒绝。带 Origin（含 `null`）的请求始终拒绝；VS Code Copilot 等 Electron 原生客户端可携带无 Origin 的 `Sec-Fetch-Site: none`、`Sec-Fetch-Dest: empty` 和 API fetch 模式（`no-cors` / `cors` / `same-origin`）。这三个头须齐全且各只有一个值，网页来源、导航请求及歧义头仍拒绝，不开放浏览器 CORS。这些本机 Fetch Metadata 头不会转发上游。teeproxy API key 校验和 Host 限制照常执行。teeproxy 不按传入 Host 选择上游，不允许客户端改变上游目标。

`client_key_env` 必填，API key 必须是 16–256 个可见 ASCII 字符；空配置无法启动代理。所有 Agent 接口（包括模型目录）要求请求携带与配置完全一致的 API key，缺失、空值或错误值返回 401，不查询管理平台或连接 Router。请求 Host 必须匹配接受连接的实际本机 IP 和端口；`localhost` 仅对回环目标有效，通配监听不会放宽 Host 检查。teeproxy 清理 agent 认证、Cookie 和逐跳头，再设置固定 router key。额外自定义头仍可能转发，请勿在其中放其他服务密钥。程序不会记录请求体、回答或认证值，响应强制 Cache-Control: no-store。

Windows 桌面版的密钥由 DPAPI 以当前用户范围加密保存，运行时在本机内存中使用。Agent 接入中的 API key 自动加载并完整显示，只用于本机校验，不上传云端。登录后自动领取的内部 Router 连接凭据与本地 API key 独立，不作为表单字段展示；平台加密保管该内部身份用于同步，登录成功后用户名和密码加密保存在本机 login-passwords.dpapi，默认掩码显示，可由用户切换为明文；退出保留已记住的密码，修改密码成功后更新。登录会话和内部身份单独保存到便携目录的 identity.dpapi，财务记录不落盘。退出或登录失效会停止旧身份的代理、删除本地登录状态并清空私有列表，本地 API key 与监听设置保留。运行记录不包含业务内容或密钥。停止操作等待监听及已有客户端连接关闭后才允许重启。最小化或关闭窗口会隐藏到系统托盘并继续转发；在托盘菜单选择“退出 teeproxy”才会结束后台运行。托盘不可用时，关闭窗口会退出程序。

router 及云平台仍须提供匹配批准策略的启动链。TEE 证明不代表供应商实际执行了某个模型，也不等于上游零日志。项目自带空批准策略；测试与历史签名样本不构成对任何实际部署的审批或生产安全审计。

Login issues a separate, limited-scope Router admission credential. The desktop client encrypts it with the remembered identity and renews it through the management session, including while minimized. Only this admission token is sent in `X-Router-Access` before attestation; business credentials remain restricted to the verified TLS session. Health negotiation uses the admission token too. CLI users supply it via `router_access_env`. It never becomes an Agent API key or an upstream header.

Agent 的 `/v1/models` 仅返回标准字段 `id`、`object`、`created`、`owned_by`，相同模型 ID 去重。商户、账号池、账号、倍率、报价和缓存状态保留在客户端界面，不发给 Agent。生成响应移除 `X-Router-*`、`X-Teeproxy-*`、Cookie 和服务端实现信息，保留原始正文、SSE、usage、错误状态、请求 ID 和限流重试头。


Router 模型请求另外要求 teeproxy 自己生成的 Ed25519 签名，覆盖正文、路由规则、请求头、时间、随机数和当前 Router 启动标识。私钥仅存储在 portable 数据目录的 `user-signing-*.dpapi`，平台只能获得公钥和签名。Router 用户身份由公钥摘要确定；仅持平台 Bearer 不能冒充此身份。退出登录保留签名凭据，换设备需恢复原签名身份，不能用重新登录静默替换它。会话与缓存继续按签名身份隔离；财务账户和历史商户限制通过独立映射保留。
