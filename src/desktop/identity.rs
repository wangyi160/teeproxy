//! Portable, Windows-user-encrypted login state, independent of local listener settings.
use super::{account_client, settings};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{io::Write, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use zeroize::Zeroizing;

pub(super) fn device(directory: &Path) -> Result<String> {
    let path = directory.join("device-id");
    if path.exists() {
        let id = std::fs::read_to_string(path)?;
        ensure!(
            id.len() == 48 && id.bytes().all(|c| c.is_ascii_hexdigit()),
            "设备标识无效"
        );
        return Ok(id);
    }
    let id = account_client::operation_id()?;
    write(&path, id.as_bytes())?;
    Ok(id)
}
pub(super) fn signer(directory: &Path, platform: &str, account: &str) -> Result<crate::user_auth::Signer> {
    use sha2::{Digest,Sha256};
    ensure!(!account.is_empty(), "请先登录账户");
    let name = crate::request_proof::hex(&Sha256::digest(serde_json::to_vec(&(platform,account))?));
    let path = directory.join(format!("user-signing-{name}.dpapi"));
    let seed = if path.exists() { settings::crypt(&std::fs::read(&path)?,false)? } else {
        let mut seed=Zeroizing::new(vec![0u8;32]);
        getrandom::getrandom(&mut seed).map_err(|_|anyhow::anyhow!("无法生成本地签名凭据"))?;
        write(&path,&settings::crypt(&seed,true)?)?;
        seed
    };
    crate::user_auth::Signer::from_seed(&seed)
}
pub(super) fn load(directory: &Path) -> Result<Option<Value>> {
    let path = directory.join("identity.dpapi");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    ensure!(bytes.len() < 128 * 1024, "登录信息过大");
    let plaintext = settings::crypt(&bytes, false)?;
    Ok(Some(serde_json::from_slice(&plaintext)?))
}
pub(super) fn save(directory: &Path, value: &Value) -> Result<()> {
    let bytes = Zeroizing::new(serde_json::to_vec(value)?);
    write(
        &directory.join("identity.dpapi"),
        &settings::crypt(&bytes, true)?,
    )
}
pub(super) fn clear(directory: &Path) -> Result<()> {
    let path = directory.join("identity.dpapi");
    match std::fs::remove_file(path) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
        _ => Ok(()),
    }
}

pub(super) fn login_password(
    directory: &Path,
    platform: &str,
    username: &str,
) -> Result<Option<(String, Zeroizing<String>)>> {
    let path = directory.join("login-passwords.dpapi");
    if !path.exists() {
        return Ok(None);
    }
    let bytes = std::fs::read(path)?;
    ensure!(bytes.len() < 128 * 1024, "登录信息过大");
    let plaintext = settings::crypt(&bytes, false)?;
    let value: Value = serde_json::from_slice(&plaintext)?;
    let result = value["accounts"]
        .as_array()
        .and_then(|rows| {
            rows.iter().rev().find(|r| {
                r["platform"] == platform
                    && (username.is_empty() || r["username"] == username.to_lowercase())
            })
        })
        .and_then(|r| {
            Some((
                r["username"].as_str()?.to_owned(),
                Zeroizing::new(r["password"].as_str()?.to_owned()),
            ))
        });
    Ok(result)
}

pub(super) fn remember_password(
    directory: &Path,
    platform: &str,
    username: &str,
    password: &str,
) -> Result<()> {
    ensure!(
        !username.is_empty() && !password.is_empty() && password.len() <= 2048,
        "登录信息无效"
    );
    let path = directory.join("login-passwords.dpapi");
    let mut rows: Vec<Value> = if path.exists() {
        let bytes = std::fs::read(&path)?;
        ensure!(bytes.len() < 128 * 1024, "登录信息过大");
        let plaintext = settings::crypt(&bytes, false)?;
        serde_json::from_slice::<Value>(&plaintext)?["accounts"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let username = username.to_lowercase();
    rows.retain(|r| !(r["platform"] == platform && r["username"] == username));
    rows.push(serde_json::json!({"platform":platform,"username":username,"password":password}));
    if rows.len() > 64 {
        rows.drain(..rows.len() - 64);
    }
    let plaintext = Zeroizing::new(serde_json::to_vec(&serde_json::json!({"accounts":rows}))?);
    write(&path, &settings::crypt(&plaintext, true)?)
}
fn write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().unwrap())?;
    let temporary = path.with_extension(format!("{}.tmp", account_client::operation_id()?));
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)?;
    let result = (|| -> Result<()> {
        file.write_all(bytes)?;
        file.sync_all()?;
        drop(file);
        let from: Vec<_> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
        let to: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
        ensure!(
            unsafe {
                MoveFileExW(
                    from.as_ptr(),
                    to.as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } != 0,
            "无法保存登录信息"
        );
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_signer_is_account_scoped_and_survives_logout() {
        let root=std::env::temp_dir().join(format!("teeproxy-signing-{}",account_client::operation_id().unwrap()));
        let a=signer(&root,"https://platform.example","alice").unwrap();
        clear(&root).unwrap();
        assert_eq!(signer(&root,"https://platform.example","alice").unwrap().public_key(),a.public_key());
        assert_ne!(signer(&root,"https://platform.example","bob").unwrap().public_key(),a.public_key());
        assert_ne!(signer(&root,"https://other.example","alice").unwrap().public_key(),a.public_key());
        let body=a.enrollment("alice","a1");
        assert_eq!(body.as_object().unwrap().len(),3);
        assert!(body.get("private_key").is_none());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn passwords_are_encrypted_scoped_and_survive_session_logout() {
        let root = std::env::temp_dir().join(format!(
            "teeproxy-passwords-{}",
            account_client::operation_id().unwrap()
        ));
        remember_password(&root, "https://platform.example", "Alice", "private-alice").unwrap();
        remember_password(&root, "https://platform.example", "Bob", "private-bob").unwrap();
        assert_eq!(
            login_password(&root, "https://platform.example", "alice")
                .unwrap()
                .unwrap()
                .1
                .as_str(),
            "private-alice"
        );
        assert_eq!(
            login_password(&root, "https://platform.example", "")
                .unwrap()
                .unwrap()
                .0,
            "bob"
        );
        assert!(
            login_password(&root, "https://other.example", "alice")
                .unwrap()
                .is_none()
        );
        assert!(
            !String::from_utf8_lossy(&std::fs::read(root.join("login-passwords.dpapi")).unwrap())
                .contains("private-alice")
        );
        clear(&root).unwrap();
        assert_eq!(
            login_password(&root, "https://platform.example", "bob")
                .unwrap()
                .unwrap()
                .1
                .as_str(),
            "private-bob"
        );
        remember_password(
            &root,
            "https://platform.example",
            "alice",
            "new-private-alice",
        )
        .unwrap();
        assert_eq!(
            login_password(&root, "https://platform.example", "alice")
                .unwrap()
                .unwrap()
                .1
                .as_str(),
            "new-private-alice"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn login_state_is_portable_encrypted_and_separate_from_local_api_key() {
        let root = std::env::temp_dir().join(format!(
            "teeproxy-identity-{}",
            account_client::operation_id().unwrap()
        ));
        let device = device(&root).unwrap();
        assert_eq!(device, super::device(&root).unwrap());
        let value = serde_json::json!({"account":{"id":"one"},"credential":"private-connection-credential","session":{"cookie":"private-session"}});
        save(&root, &value).unwrap();
        assert_eq!(load(&root).unwrap().unwrap(), value);
        let encoded = std::fs::read(root.join("identity.dpapi")).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("private-connection"));
        clear(&root).unwrap();
        assert!(load(&root).unwrap().is_none());
        assert!(root.join("device-id").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}
