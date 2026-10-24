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
