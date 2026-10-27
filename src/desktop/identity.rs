//! Portable, Windows-user-encrypted login state, independent of local listener settings.
use super::{account_client, settings};
use anyhow::{Result, ensure};
use serde_json::Value;
use std::{io::Write, os::windows::ffi::OsStrExt, path::Path};
use windows_sys::Win32::Storage::FileSystem::{
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
};
use zeroize::Zeroizing;

pub(super) fn create(directory: &Path, platform: &str) -> Result<String> {
    let mut seed=Zeroizing::new(vec![0;32]);
    getrandom::getrandom(&mut seed).map_err(|_|anyhow::anyhow!("无法生成身份"))?;
    store_key(directory,platform,&seed)
}
fn store_key(directory: &Path, platform: &str, seed: &[u8]) -> Result<String> {
    let signer=crate::user_auth::Signer::from_seed(seed)?;
    let id=signer.user_id();
    let value=serde_json::json!({"version":1,"platform":platform,"identity_id":id,"seed":crate::request_proof::hex(seed)});
    let bytes=Zeroizing::new(serde_json::to_vec(&value)?);
    write(&directory.join("identities").join(format!("{id}.dpapi")),&settings::crypt(&bytes,true)?)?;
    Ok(id)
}
pub(super) fn key(directory:&Path, platform:&str, id:&str)->Result<crate::user_auth::Signer>{
    ensure!(id.starts_with("u_sig_") && id.len()==70 && id[6..].bytes().all(|b|b.is_ascii_hexdigit()),"身份 ID 无效");
    let bytes=settings::crypt(&std::fs::read(directory.join("identities").join(format!("{id}.dpapi")))?,false)?;
    let value:Value=serde_json::from_slice(&bytes)?;
    ensure!(value["platform"]==platform&&value["identity_id"]==id,"身份平台不匹配");
    let seed=Zeroizing::new(crate::request_proof::unhex(value["seed"].as_str().unwrap_or(""),32).map_err(anyhow::Error::msg)?);
    let signer=crate::user_auth::Signer::from_seed(&seed)?;
    ensure!(signer.user_id()==id,"身份文件损坏");Ok(signer)
}
pub(super) fn identities(directory:&Path,platform:&str)->Result<Vec<String>>{
    let mut ids=Vec::new();
    let path=directory.join("identities");if !path.exists(){return Ok(ids);}
    for entry in std::fs::read_dir(path)? {
        let path=entry?.path();
        if path.extension().and_then(|s|s.to_str())!=Some("dpapi"){continue;}
        let id=path.file_stem().and_then(|s|s.to_str()).unwrap_or("");
        // A different platform's identities are not selectable on this platform.
        if key(directory,platform,id).is_ok(){ids.push(id.to_owned());}
    }
    ids.sort();Ok(ids)
}
pub(super) fn migrate(directory:&Path,saved:&Value)->Result<String>{
    let platform=saved["platform"].as_str().ok_or_else(||anyhow::anyhow!("平台地址无效"))?;
    if let Some(id)=saved["identity_id"].as_str(){key(directory,platform,id)?;return Ok(id.to_owned());}
    use sha2::{Digest,Sha256};
    let account=saved["account"]["id"].as_str().ok_or_else(||anyhow::anyhow!("账户信息无效"))?;
    let name=crate::request_proof::hex(&Sha256::digest(serde_json::to_vec(&(platform,account))?));
    // Never generate a replacement for a missing existing key.
    let seed=settings::crypt(&std::fs::read(directory.join(format!("user-signing-{name}.dpapi")))?,false)?;
    let signer=crate::user_auth::Signer::from_seed(&seed)?;
    ensure!(saved["public_key"]==signer.public_key(),"请恢复原身份密钥文件");
    store_key(directory,platform,&seed)
}

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
    fn independent_ids_survive_logout_and_legacy_migration_preserves_the_key() {
        let root=std::env::temp_dir().join(format!("teeproxy-identities-{}",account_client::operation_id().unwrap()));
        let base="https://platform.example";
        let original=signer(&root,base,"old-account").unwrap();
        let saved=serde_json::json!({"platform":base,"account":{"id":"old-account"},"public_key":original.public_key()});
        let migrated=migrate(&root,&saved).unwrap();
        assert_eq!(key(&root,base,&migrated).unwrap().public_key(),original.public_key());
        for _ in 0..70 {create(&root,base).unwrap();}
        clear(&root).unwrap();
        let all=identities(&root,base).unwrap();
        assert_eq!(all.len(),71);assert!(all.contains(&migrated));
        assert!(identities(&root,"https://other.example").unwrap().is_empty());
        assert!(key(&root,base,"../identity").is_err());
        let mut changed=saved;changed["public_key"]=serde_json::json!("00".repeat(32));
        assert!(migrate(&root,&changed).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
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
