use crate::config::Config;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    ptr,
};
use windows_sys::Win32::{
    Foundation::LocalFree,
    Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
    },
    Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW},
};
use zeroize::{Zeroize, Zeroizing};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Profile {
    pub router: String,
    pub listen_ip: String,
    pub port: u16,
    pub tdx_policy: String,
    pub snp_policy: String,
    pub pool: String,
    pub routing: crate::routing::Policy,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            router: String::new(),
            listen_ip: "127.0.0.1".into(),
            port: 8899,
            tdx_policy: String::new(),
            snp_policy: String::new(),
            pool: String::new(),
            routing: Default::default(),
        }
    }
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Secrets {
    pub router_key: String,
    pub client_key: String,
}
impl Drop for Secrets {
    fn drop(&mut self) {
        self.router_key.zeroize();
        self.client_key.zeroize();
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Saved {
    version: u32,
    profile: Profile,
    protected_secrets: Vec<u8>,
}
impl Profile {
    pub fn config(&self, secrets: &Secrets, directory: &Path) -> Result<Config> {
        ensure!(self.port != 0, "本地端口必须介于 1 和 65535 之间");
        let ip: std::net::IpAddr = self
            .listen_ip
            .trim()
            .parse()
            .context("监听 IP 必须是有效的 IPv4 或 IPv6 地址")?;
        ensure!(
            ip.is_loopback() || !secrets.client_key.is_empty(),
            "监听局域网或所有网卡时，请设置 API key"
        );
        ensure!(
            secrets.client_key.is_empty()
                || (16..=256).contains(&secrets.client_key.len())
                    && secrets.client_key.bytes().all(|c| c.is_ascii_graphic()),
            "API key 须为 16–256 个可见 ASCII 字符"
        );
        let policy = |value: &str| (!value.trim().is_empty()).then(|| directory.join(value.trim()));
        let config = Config {
            listen: (ip, self.port).into(),
            router: self.router.trim().to_owned(),
            router_key_env: "TEEPROXY_ROUTER_KEY".into(),
            tdx_policy: policy(&self.tdx_policy),
            snp_policy: policy(&self.snp_policy),
            client_key_env: (!secrets.client_key.is_empty()).then(|| "TEEPROXY_CLIENT_KEY".into()),
            pool: (!self.pool.trim().is_empty()).then(|| self.pool.trim().to_owned()),
            routing: self.routing.clone(),
            max_in_flight: 16,
            max_connections: 128,
            response_header_timeout_secs: crate::config::response_header_seconds(),
            stream_idle_secs: 120,
        };
        config.validate()?;
        Ok(config)
    }
}
pub fn default_path() -> Result<PathBuf> {
    let executable = std::env::current_exe().context("无法定位 teeproxy 程序目录")?;
    let path = portable_path(&executable)?;
    if let Some(legacy) = std::env::var_os("LOCALAPPDATA") {
        migrate_legacy(&path, &PathBuf::from(legacy).join("teeproxy/desktop.json"))?;
    }
    Ok(path)
}
fn portable_path(executable: &Path) -> Result<PathBuf> {
    Ok(executable
        .parent()
        .context("无法定位 teeproxy 程序目录")?
        .join("data/desktop.json"))
}
fn migrate_legacy(path: &Path, legacy: &Path) -> Result<()> {
    // An existing portable profile is authoritative, including an unreadable one.
    // Keep the old files as a backup; never fall back to writing AppData.
    if path.try_exists()? || !legacy.try_exists()? {
        return Ok(());
    }
    let (profile, secrets) = load(legacy).context("无法迁移旧版桌面配置")?;
    save_from(
        path,
        &profile,
        &secrets,
        legacy.parent().context("旧版配置路径缺少目录")?,
        false,
    )
    .context("无法将旧版配置迁移到程序 data 目录")?;
    Ok(())
}
pub fn load(path: &Path) -> Result<(Profile, Secrets)> {
    if !path.exists() {
        return Ok((Profile::default(), Secrets::default()));
    }
    let bytes = std::fs::read(path).context("无法读取桌面配置")?;
    ensure!(bytes.len() <= 128 * 1024, "桌面配置文件过大");
    let saved: Saved = serde_json::from_slice(&bytes).context("桌面配置格式无效")?;
    ensure!(saved.version == 1, "不支持的桌面配置版本");
    let plaintext = crypt(&saved.protected_secrets, false)?;
    let secrets = serde_json::from_slice(&plaintext).context("密钥数据无效")?;
    Ok((saved.profile, secrets))
}
pub fn save(path: &Path, profile: &Profile, secrets: &Secrets) -> Result<Profile> {
    save_from(
        path,
        profile,
        secrets,
        path.parent().context("配置路径缺少目录")?,
        true,
    )
}
fn save_from(
    path: &Path,
    profile: &Profile,
    secrets: &Secrets,
    source_directory: &Path,
    replace: bool,
) -> Result<Profile> {
    let directory = path.parent().context("配置路径缺少目录")?;
    profile.config(secrets, source_directory)?;
    ensure!(
        secrets.router_key.len() <= 8192 && secrets.client_key.len() <= 256,
        "密钥过长"
    );
    let plaintext = Zeroizing::new(serde_json::to_vec(secrets)?);
    let protected_secrets = crypt(&plaintext, true)?.to_vec();
    std::fs::create_dir_all(directory).context("无法写入配置目录，请将程序解压到可写目录")?;
    let mut profile = profile.clone();
    profile.tdx_policy = import_policy(&profile.tdx_policy, source_directory, directory, "tdx")?;
    profile.snp_policy = import_policy(&profile.snp_policy, source_directory, directory, "snp")?;
    let bytes = serde_json::to_vec_pretty(&Saved {
        version: 1,
        profile: profile.clone(),
        protected_secrets,
    })?;
    let temporary = directory.join(format!(".desktop-{}.tmp", crate::protocol::random_hex()?));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let written = file.write_all(&bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = written {
        let _ = std::fs::remove_file(&temporary);
        return Err(error).context("无法保存桌面配置");
    }
    let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both strings are terminated UTF-16; MoveFileEx atomically replaces this file.
    let ok = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_WRITE_THROUGH
                | if replace {
                    MOVEFILE_REPLACE_EXISTING
                } else {
                    0
                },
        )
    };
    if ok == 0 {
        let error = std::io::Error::last_os_error();
        let _ = std::fs::remove_file(&temporary);
        return Err(error).context("无法保存桌面配置");
    }
    Ok(profile)
}

fn import_policy(value: &str, source: &Path, destination: &Path, kind: &str) -> Result<String> {
    if value.trim().is_empty() {
        return Ok(String::new());
    }
    let root = destination.canonicalize()?;
    let original = source
        .join(value.trim())
        .canonicalize()
        .with_context(|| format!("无法读取 {} 策略文件", kind.to_uppercase()))?;
    ensure!(original.is_file(), "策略路径必须指向文件");
    if let Ok(relative) = original.strip_prefix(&root) {
        return Ok(relative.to_string_lossy().into_owned());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&original)?
        .take(512 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 512 * 1024, "策略文件过大");
    let policies = root.join("policies");
    std::fs::create_dir_all(&policies)?;
    ensure!(
        policies.canonicalize()?.starts_with(&root),
        "策略目录必须位于配置目录内"
    );
    let name = original.file_name().context("策略文件缺少文件名")?;
    let mut target = policies.join(name);
    if target.try_exists()? && std::fs::read(&target)? != bytes {
        use sha2::{Digest, Sha256};
        target = policies.join(format!("{kind}-{:x}.json", Sha256::digest(&bytes)));
    }
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
    {
        Ok(mut file) => {
            let written = file.write_all(&bytes).and_then(|()| file.sync_all());
            drop(file);
            if let Err(error) = written {
                let _ = std::fs::remove_file(&target);
                return Err(error).context("无法保存本地策略副本");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            ensure!(
                target.canonicalize()?.starts_with(&root) && std::fs::read(&target)? == bytes,
                "本地策略副本冲突"
            );
        }
        Err(error) => return Err(error).context("无法保存本地策略副本"),
    }
    Ok(target.strip_prefix(root)?.to_string_lossy().into_owned())
}
use std::os::windows::ffi::OsStrExt;

fn crypt(bytes: &[u8], protect: bool) -> Result<Zeroizing<Vec<u8>>> {
    ensure!(bytes.len() <= 64 * 1024, "密钥数据过大");
    let input = CRYPT_INTEGER_BLOB {
        cbData: bytes.len() as u32,
        pbData: bytes.as_ptr().cast_mut(),
    };
    let mut output: CRYPT_INTEGER_BLOB = unsafe { std::mem::zeroed() };
    // DPAPI receives valid blobs and allocates its own output. No machine-wide flag.
    let ok = unsafe {
        if protect {
            CryptProtectData(
                &input,
                ptr::null(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        } else {
            CryptUnprotectData(
                &input,
                ptr::null_mut(),
                ptr::null(),
                ptr::null(),
                ptr::null(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut output,
            )
        }
    };
    ensure!(
        ok != 0,
        "Windows 无法读取或加密密钥；请使用原 Windows 用户账户"
    );
    // Copy to a zeroizing buffer, clear decrypted API storage, then free with LocalFree.
    let result = unsafe {
        if output.cbData == 0 {
            LocalFree(output.pbData.cast());
            return Ok(Zeroizing::new(Vec::new()));
        }
        let slice = std::slice::from_raw_parts_mut(output.pbData, output.cbData as usize);
        let result = Zeroizing::new(slice.to_vec());
        if !protect {
            slice.zeroize();
        }
        LocalFree(output.pbData.cast());
        result
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestDirectory(PathBuf);
    impl TestDirectory {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "teeproxy-portable-{}",
                crate::protocol::random_hex().unwrap()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TestDirectory {
        fn drop(&mut self) {
            let target = self.0.canonicalize().unwrap();
            assert!(target.starts_with(std::env::temp_dir().canonicalize().unwrap()));
            assert!(
                target
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("teeproxy-portable-")
            );
            std::fs::remove_dir_all(target).unwrap();
        }
    }

    fn fixture_profile() -> Profile {
        Profile {
            router: "https://router.example".into(),
            routing: crate::routing::Policy {
                automatic: true,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn portable_migration_preserves_keys_policies_and_survives_folder_move() {
        let temp = TestDirectory::new();
        let legacy = temp.0.join("old-user-profile");
        std::fs::create_dir(&legacy).unwrap();
        let tdx = legacy.join("phala-tdx.json");
        let snp = legacy.join("snp.json");
        std::fs::write(&tdx, b"reviewed-tdx-policy").unwrap();
        std::fs::write(&snp, b"reviewed-snp-policy").unwrap();
        let mut profile = fixture_profile();
        profile.tdx_policy = tdx.to_str().unwrap().into();
        profile.snp_policy = "snp.json".into();
        let keys = Secrets {
            router_key: "private-router-token".into(),
            client_key: "private-local-token".into(),
        };
        // Old versions could store absolute and relative external paths.
        let legacy_file = legacy.join("desktop.json");
        let old = serde_json::to_vec(&Saved {
            version: 1,
            profile,
            protected_secrets: crypt(&serde_json::to_vec(&keys).unwrap(), true)
                .unwrap()
                .to_vec(),
        })
        .unwrap();
        std::fs::write(&legacy_file, &old).unwrap();
        let exe = temp.0.join("portable/teeproxy.exe");
        let portable = portable_path(&exe).unwrap();
        assert_eq!(portable, temp.0.join("portable/data/desktop.json"));
        migrate_legacy(&portable, &legacy_file).unwrap();
        assert_eq!(std::fs::read(&legacy_file).unwrap(), old);
        let (saved, decrypted) = load(&portable).unwrap();
        assert_eq!(decrypted.router_key, keys.router_key);
        assert_eq!(decrypted.client_key, keys.client_key);
        assert!(saved.routing.automatic);
        for value in [&saved.tdx_policy, &saved.snp_policy] {
            assert!(Path::new(value).is_relative());
            assert!(Path::new(value).starts_with("policies"));
        }
        assert!(
            !std::fs::read_to_string(&portable)
                .unwrap()
                .contains(&keys.router_key)
        );
        // A different launch directory or moving the app must not change resolution.
        let moved = temp.0.join("moved-application");
        std::fs::rename(exe.parent().unwrap(), &moved).unwrap();
        std::fs::remove_file(&tdx).unwrap();
        std::fs::remove_file(&snp).unwrap();
        let moved_profile = moved.join("data/desktop.json");
        let (saved, decrypted) = load(&moved_profile).unwrap();
        let config = saved
            .config(&decrypted, moved_profile.parent().unwrap())
            .unwrap();
        assert_eq!(
            std::fs::read(config.tdx_policy.unwrap()).unwrap(),
            b"reviewed-tdx-policy"
        );
        assert_eq!(
            std::fs::read(config.snp_policy.unwrap()).unwrap(),
            b"reviewed-snp-policy"
        );
        // Existing portable profiles are never replaced by old AppData settings.
        let before = std::fs::read(&moved_profile).unwrap();
        std::fs::write(&legacy_file, b"invalid-old-profile").unwrap();
        migrate_legacy(&moved_profile, &legacy_file).unwrap();
        assert_eq!(std::fs::read(&moved_profile).unwrap(), before);
    }

    #[test]
    fn external_policy_copy_is_relative_collision_safe_and_failure_keeps_saved_profile() {
        let temp = TestDirectory::new();
        let external = temp.0.join("outside.json");
        std::fs::write(&external, b"original-policy").unwrap();
        let path = temp.0.join("application/data/desktop.json");
        let mut profile = fixture_profile();
        profile.tdx_policy = external.to_str().unwrap().into();
        let keys = Secrets::default();
        let first = save(&path, &profile, &keys).unwrap();
        let base = path.parent().unwrap();
        assert_eq!(
            Path::new(&first.tdx_policy),
            Path::new("policies/outside.json")
        );
        // Same-name policy changes get a new file; the old approved bytes stay intact.
        std::fs::write(&external, b"new-policy").unwrap();
        let second = save(&path, &profile, &keys).unwrap();
        assert_ne!(first.tdx_policy, second.tdx_policy);
        assert_eq!(
            std::fs::read(base.join(&first.tdx_policy)).unwrap(),
            b"original-policy"
        );
        assert_eq!(
            std::fs::read(base.join(&second.tdx_policy)).unwrap(),
            b"new-policy"
        );
        assert_eq!(
            save(&path, &profile, &keys).unwrap().tdx_policy,
            second.tdx_policy
        );
        let before = std::fs::read(&path).unwrap();
        profile.snp_policy = "missing.json".into();
        assert!(save(&path, &profile, &keys).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::read(&external).unwrap(), b"new-policy");
    }

    #[test]
    fn failed_migration_does_not_create_portable_profile_or_overwrite_existing_one() {
        let temp = TestDirectory::new();
        let legacy = temp.0.join("legacy.json");
        let path = temp.0.join("application/data/desktop.json");
        migrate_legacy(&path, &legacy).unwrap();
        assert!(!path.exists());
        let mut profile = fixture_profile();
        profile.tdx_policy = "missing.json".into();
        let old = serde_json::to_vec(&Saved {
            version: 1,
            profile,
            protected_secrets: crypt(&serde_json::to_vec(&Secrets::default()).unwrap(), true)
                .unwrap()
                .to_vec(),
        })
        .unwrap();
        std::fs::write(&legacy, &old).unwrap();
        assert!(migrate_legacy(&path, &legacy).is_err());
        assert!(!path.exists());
        assert_eq!(std::fs::read(&legacy).unwrap(), old);
        std::fs::write(&path, b"corrupt-portable-profile").unwrap();
        migrate_legacy(&path, &legacy).unwrap();
        assert!(load(&path).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"corrupt-portable-profile");
        let blocked = temp.0.join("not-a-directory");
        std::fs::write(&blocked, b"existing-file").unwrap();
        assert!(
            save(
                &blocked.join("desktop.json"),
                &fixture_profile(),
                &Secrets::default()
            )
            .is_err()
        );
        assert_eq!(std::fs::read(blocked).unwrap(), b"existing-file");
    }

    #[test]
    fn dpapi_round_trip_and_tamper_rejection() {
        let secret = b"router-key-must-not-be-plaintext";
        let cipher = crypt(secret, true).unwrap();
        assert!(!cipher.windows(secret.len()).any(|v| v == secret));
        assert_eq!(crypt(&cipher, false).unwrap().as_slice(), secret);
        let mut corrupt = cipher.to_vec();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        assert!(crypt(&corrupt, false).is_err());
    }
    #[test]
    fn desktop_settings_preserve_address_policy_and_encrypted_secrets() {
        let dir = std::env::temp_dir().join(format!(
            "teeproxy-settings-{}",
            crate::protocol::random_hex().unwrap()
        ));
        let path = dir.join("desktop.json");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("approved.json"), b"{}").unwrap();
        let profile = Profile {
            router: "https://router.invalid:8443".into(),
            listen_ip: "0.0.0.0".into(),
            tdx_policy: "approved.json".into(),
            ..Profile::default()
        };
        let secrets = Secrets {
            router_key: "test-router-key".into(),
            client_key: "test-local-key-123456".into(),
        };
        save(&path, &profile, &secrets).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(!text.contains(&secrets.router_key) && !text.contains(&secrets.client_key));
        let (loaded, keys) = load(&path).unwrap();
        assert_eq!(loaded.router, profile.router);
        assert_eq!(loaded.listen_ip, "0.0.0.0");
        assert!(
            loaded
                .config(&keys, &dir)
                .unwrap()
                .listen
                .ip()
                .is_unspecified()
        );
        assert_eq!(keys.router_key, secrets.router_key);
        assert_eq!(
            loaded.config(&keys, &dir).unwrap().tdx_policy.unwrap(),
            dir.join("approved.json")
        );
        std::fs::write(&path, b"{}").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_file(dir.join("approved.json")).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }

    #[test]
    fn legacy_profiles_remain_local_and_network_binds_require_a_valid_key() {
        let mut profile: Profile =
            serde_json::from_str(r#"{"router":"https://router.invalid","port":8899}"#).unwrap();
        let mut secrets = Secrets::default();
        assert_eq!(profile.listen_ip, "127.0.0.1");
        assert_eq!(
            crate::config::local_api_url(profile.config(&secrets, Path::new(".")).unwrap().listen),
            "http://127.0.0.1:8899/v1"
        );
        for ip in ["0.0.0.0", "192.168.1.6", "::"] {
            profile.listen_ip = ip.into();
            secrets.client_key.clear();
            assert!(profile.config(&secrets, Path::new(".")).is_err());
            secrets.client_key = "too-short".into();
            assert!(profile.config(&secrets, Path::new(".")).is_err());
            secrets.client_key = "test-local-client-key".into();
            profile.config(&secrets, Path::new(".")).unwrap();
        }
        profile.listen_ip = "::1".into();
        secrets.client_key.clear();
        assert_eq!(
            crate::config::local_api_url(profile.config(&secrets, Path::new(".")).unwrap().listen),
            "http://[::1]:8899/v1"
        );
        for ip in [
            "localhost",
            "127.0.0.1:8899",
            "http://192.168.1.6",
            "invalid",
        ] {
            profile.listen_ip = ip.into();
            assert!(profile.config(&secrets, Path::new(".")).is_err());
        }
    }
}
