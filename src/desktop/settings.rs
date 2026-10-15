use crate::config::Config;
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
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
        let policy = |value: &str| (!value.trim().is_empty()).then(|| directory.join(value.trim()));
        let config = Config {
            listen: ([127, 0, 0, 1], self.port).into(),
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
    let directory = std::env::var_os("LOCALAPPDATA").context("无法定位 Windows 用户配置目录")?;
    Ok(PathBuf::from(directory).join("teeproxy/desktop.json"))
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
pub fn save(path: &Path, profile: &Profile, secrets: &Secrets) -> Result<()> {
    let directory = path.parent().context("配置路径缺少目录")?;
    profile.config(secrets, directory)?;
    ensure!(
        secrets.router_key.len() <= 8192 && secrets.client_key.len() <= 256,
        "密钥过长"
    );
    let plaintext = Zeroizing::new(serde_json::to_vec(secrets)?);
    let protected_secrets = crypt(&plaintext, true)?.to_vec();
    let bytes = serde_json::to_vec_pretty(&Saved {
        version: 1,
        profile: profile.clone(),
        protected_secrets,
    })?;
    std::fs::create_dir_all(directory).context("无法创建配置目录")?;
    let temporary = directory.join(format!(".desktop-{}.tmp", crate::protocol::random_hex()?));
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    drop(file);
    let from: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both strings are terminated UTF-16; MoveFileEx atomically replaces this file.
    let ok = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        let error = std::io::Error::last_os_error();
        let _ = std::fs::remove_file(&temporary);
        return Err(error).context("无法保存桌面配置");
    }
    Ok(())
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
        let profile = Profile {
            router: "https://router.invalid:8443".into(),
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
        assert_eq!(keys.router_key, secrets.router_key);
        assert_eq!(
            loaded.config(&keys, &dir).unwrap().tdx_policy.unwrap(),
            dir.join("approved.json")
        );
        std::fs::write(&path, b"{}").unwrap();
        assert!(load(&path).is_err());
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(&dir).unwrap();
    }
}
