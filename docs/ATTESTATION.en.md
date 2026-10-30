<a id="证明与本地策略"></a>

# Attestation and local policies

[简体中文](ATTESTATION.md) | **English**

Verification code lives in `src/client.rs`, `src/tdx.rs` and `src/snp.rs`; the shared wire protocol is in `src/protocol.rs`. Router's attestation endpoint is `POST /.well-known/teellm/attestation`.

1. Establish TLS 1.3 and verify the peer's handshake signature, without yet trusting the server identity or sending business credentials.
2. Generate a fresh 32-byte random challenge and request evidence over that TLS connection.
3. Check the protocol version, challenge and current TLS certificate digest.
4. Locally validate Intel/AMD hardware signatures, certificate chains, revocation material, validity and TCB, matching the independently approved measurement policy.
5. Verify the hardware REPORT_DATA binding: `SHA512("teellm:attested-channel:v1\0" || nonce32 || SHA256(certDER))`.
6. Send business requests only over that same verified connection. Disconnection or expiration of its reuse window requires verification again.

## Intel TDX / Phala dstack

`tdx_policy` points to UTF-8 JSON. See `Policy`/`Release` in `src/tdx.rs` for complete fields:

- `schema_version` is 1.
- `releases` contains independently reviewed releases, each with `id`, `mr_td`, three `rtmrs` (RTMR0–2), `event_log_version` and the complete ordered RTMR3 `events`.
- Each event has `event` and a hex `payload`; only `instance-id` may use null to allow a variable payload.
- Policies must include `compose-hash` and `key-provider` bindings before the boot boundary. Verification checks event ordering, digests, log version and replayed RTMR3.
- `allow_dynamic_platform`, `allow_cached_keys` and `allow_smt` default to false. Relaxations require the user's independent hardware-release review.

Intel collateral is fetched only from fixed Intel domains with ordinary public TLS certificate validation. Network material does not supply a trusted verdict. Measurements supplied by an evidence service are never automatically added to the allowlist.

`policy/tdx-deny-all.json` is empty. Build a policy from reviewed OS and Router builds, the final complete app-compose, KMS and boot events. Do not use synthetic test policies or historical hardware samples as production approvals.

## AMD SEV-SNP

Enable `snp-verifier` and configure `snp_policy`. Policies include validity windows and releases constraining CPU product, launch measurement, HOST_DATA, guest policy, minimum guest SVN/TCB, platform and mitigation bits. AMD roots come from the pinned sev crate; endorsements and revocation materials come only from fixed AMD HTTPS services.

HOST_DATA can be supplied by the host. Matching its hash alone does not attest the OS or application; it must be combined with reviewed measurements and an enforced boot chain. `policy/deny-all.json` is also empty and trusts no instance by default.

Policies are loaded when verification or proxy startup begins. After changes, stop/restart the desktop proxy or restart the CLI process. Online verification always uses current local time; historical test times apply only to public historical signed samples.
