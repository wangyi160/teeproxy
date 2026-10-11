# 证明与本地策略

验证代码位于 `src/client.rs`、`src/tdx.rs` 和 `src/snp.rs`；共享线协议位于 `src/protocol.rs`。router 的证明接口为 `POST /.well-known/teellm/attestation`。

1. 建立 TLS 1.3 并验证对端握手签名，但此时不认可服务器身份，也不发送凭证。
2. 生成新的 32 字节随机挑战，在该 TLS 连接上请求证明。
3. 核对协议版本、挑战和当前 TLS 证书摘要。
4. 本机验证 Intel 或 AMD 的硬件签名、证书链、吊销材料、有效期及 TCB，并匹配本地批准的测量策略。
5. 核对硬件 REPORT_DATA 中的绑定值：`SHA512("teellm:attested-channel:v1\0" || nonce32 || SHA256(certDER))`。
6. 业务请求只使用刚刚完成验证的同一连接。连接断开或复用窗口过期后重新执行验证。

## Intel TDX / Phala dstack

`tdx_policy` 指向 UTF-8 JSON。完整字段以 `src/tdx.rs` 的 `Policy`/`Release` 为准：

- `schema_version` 为 1。
- `releases` 包含用户独立审核的发布项，每项有 `id`、`mr_td`、三个 `rtmrs`（RTMR0–2）、`event_log_version` 和完整、有序的 RTMR3 `events`。
- 每条事件包含 `event` 和十六进制 `payload`；只有 `instance-id` 可用 null 表示可变载荷。
- 策略必须包含启动边界之前的 `compose-hash` 和 `key-provider` 绑定。代码校验事件顺序、摘要、日志版本及重放后的 RTMR3。
- `allow_dynamic_platform`、`allow_cached_keys`、`allow_smt` 默认 false；任何放宽都必须由用户按自己的硬件发布审核决定。

Intel collateral 只从固定 Intel 域名获取，TLS 采用正常公网证书校验；网络不提供可信验证结论。取证服务传来的测量值不能自动加入允许表。

`policy/tdx-deny-all.json` 是空策略；请从审核后的 OS、router 构建、最终完整 app-compose、KMS 和启动事件建立自己的策略。不要把 tests 中的合成策略或历史硬件样本用于生产。

## AMD SEV-SNP

启用 `snp-verifier`，设置 `snp_policy`。策略含有效时间窗口及发布项，约束 CPU 产品、launch measurement、HOST_DATA、guest policy、最低 guest SVN/TCB、平台与 mitigation 位。AMD 根来自锁定的 sev crate，endorsement 与吊销材料只从固定 AMD HTTPS 服务获取。

HOST_DATA 是宿主可提供的数据，其哈希单独匹配不能证明 OS 或应用；必须与被审核的测量和强制执行的启动链一起使用。`policy/deny-all.json` 同样为空，默认不会认可任何实例。

策略在进程启动时读取，修改后应重启 teeproxy。线上验证始终使用本机当前时间；测试中的历史时间只用于公开、历史签名样本。
