//! Management snapshots only. This module never connects to Router.
use std::{future::Future, pin::Pin, sync::Arc, time::{Duration, Instant}};
use anyhow::{Result, ensure};
use serde_json::{Value, json};
use tokio::sync::{Mutex, watch};

pub type Load = Pin<Box<dyn Future<Output=Result<Value>> + Send>>;
#[derive(Debug)]
pub struct AccessRevoked;
impl std::fmt::Display for AccessRevoked { fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f,"目录访问被拒绝（401/403），代理已停止；请重新登录或检查账户授权") } }
impl std::error::Error for AccessRevoked {}

#[derive(Default)]
struct State { value: Option<Value>, checked: Option<Instant>, revoked: bool }
pub struct Cache {
    loader: Box<dyn Fn() -> Load + Send + Sync>,
    state: Mutex<State>,
    revoked: watch::Sender<bool>,
}
impl Cache {
    pub fn new(loader: impl Fn() -> Load + Send + Sync + 'static) -> Arc<Self> {
        Arc::new(Self { loader:Box::new(loader), state:Mutex::new(State::default()), revoked:watch::channel(false).0 })
    }
    pub(crate) fn is_revoked(&self) -> bool { *self.revoked.borrow() }
    pub(crate) fn revocation(&self) -> watch::Receiver<bool> { self.revoked.subscribe() }
    pub fn public(base: &str) -> Result<Arc<Self>> {
        let url=reqwest::Url::parse(base)?;
        ensure!(url.scheme()=="https" && url.host_str().is_some() && url.username().is_empty()
            && url.password().is_none() && url.path()=="/" && url.query().is_none() && url.fragment().is_none(), "Invalid management URL");
        let endpoint=format!("{}/portal/api/catalog",base.trim_end_matches('/'));
        let http=reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(20)).build()?;
        Ok(Self::new(move || { let http=http.clone();let endpoint=endpoint.clone();Box::pin(async move {
            let mut response=http.get(endpoint).send().await?;
            if matches!(response.status().as_u16(),401|403) { return Err(AccessRevoked.into()); }
            ensure!(response.status().is_success(),"管理平台模型目录不可用");
            let mut bytes=Vec::new();
            while let Some(chunk)=response.chunk().await? {ensure!(bytes.len()+chunk.len()<=16*1024*1024,"模型目录过大");bytes.extend_from_slice(&chunk);}
            let raw:Value=serde_json::from_slice(&bytes)?;
            let mut models=std::collections::BTreeMap::new();
            for p in raw["data"].as_array().ok_or_else(||anyhow::anyhow!("模型目录格式无效"))? {
                if p["available_accounts"].as_u64().unwrap_or(0)==0 {continue;}
                for m in p["models"].as_array().into_iter().flatten() {
                    let id=m["id"].as_str().ok_or_else(||anyhow::anyhow!("模型目录格式无效"))?;
                    models.entry(id.to_owned()).or_insert_with(||json!({"id":id,"object":"model","owned_by":p["provider"]}));
                }
            }
            Ok(json!({"version":1,"source":"management_snapshot","status":raw["status"],"updated_at":raw["updated_at"],
                "models":{"object":"list","data":models.into_values().collect::<Vec<_>>()},"pools":{"object":"list","data":[]}}))
        }) }))
    }
    pub async fn load(&self, force: bool) -> Result<Value> {
        // One fetch per cache interval, including concurrent /models calls.
        let started=Instant::now();
        let mut state=self.state.lock().await;
        // Revocation is final for this identity's cache. A new authenticated
        // connection creates a new cache; a late refresh cannot undo a denial.
        if state.revoked { return Err(AccessRevoked.into()); }
        if let Some(at)=state.checked {
            if (!force && at.elapsed()<Duration::from_secs(300)) || at>=started {
                if let Some(value)=&state.value {return Ok(value.clone());}
                if state.revoked { return Err(AccessRevoked.into()); }
                anyhow::bail!("管理平台模型目录暂不可用");
            }
        }
        let result=(self.loader)().await.and_then(|value| {
            ensure!(value["version"]==1 && value["source"]=="management_snapshot"
                && value["models"]["data"].is_array() && value["pools"]["data"].is_array(),"管理平台目录格式无效");
            Ok(value)
        });
        state.checked=Some(Instant::now());
        match result {
            Ok(value)=>{state.revoked=false;state.value=Some(value.clone());Ok(value)}
            Err(error)=>{
                if error.downcast_ref::<AccessRevoked>().is_some() {
                    state.value=None;state.revoked=true;self.revoked.send_replace(true);return Err(error);
                }
                if let Some(value)=&mut state.value {value["status"]=json!("stale");return Ok(value.clone());}
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize,Ordering};
    #[tokio::test]
    async fn coalesces_reads_preserves_cache_offline_and_clears_on_revocation() {
        let calls=Arc::new(AtomicUsize::new(0));let hits=calls.clone();
        let cache=Cache::new(move || {let hits=hits.clone();Box::pin(async move {
            match hits.fetch_add(1,Ordering::SeqCst) {
                0=>Ok(json!({"version":1,"source":"management_snapshot","models":{"data":[{"id":"grok/test"}]},"pools":{"data":[]}})),
                1=>anyhow::bail!("offline"),
                _=>Err(AccessRevoked.into())
            }
        })});
        let (a,b)=tokio::join!(cache.load(false),cache.load(false));
        assert_eq!(a.unwrap(),b.unwrap());assert_eq!(calls.load(Ordering::SeqCst),1);
        assert_eq!(cache.load(true).await.unwrap()["status"],"stale");
        assert!(cache.load(true).await.unwrap_err().is::<AccessRevoked>());
        assert!(cache.load(false).await.unwrap_err().is::<AccessRevoked>());
        assert!(cache.load(true).await.unwrap_err().is::<AccessRevoked>());
        assert!(cache.is_revoked());
        assert_eq!(calls.load(Ordering::SeqCst),3);
        let fresh=Cache::new(||Box::pin(async {anyhow::bail!("offline")}));
        assert!(fresh.load(false).await.is_err());
        assert!(!fresh.is_revoked());
    }
}
