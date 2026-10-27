//! A client-owned identity. Only the public key and request proofs leave this process.
use anyhow::{Result, ensure};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::sync::Arc;
use crate::request_proof as wire;
#[derive(Clone)]
pub struct Signer(Arc<Ed25519KeyPair>);
impl Signer {
    pub fn from_seed(seed: &[u8]) -> Result<Self> {
        Ok(Self(Arc::new(Ed25519KeyPair::from_seed_unchecked(seed).map_err(|_|anyhow::anyhow!("Invalid local signing identity"))?)))
    }
    pub fn public_key(&self)->String {wire::hex(self.0.public_key().as_ref())}
    pub fn user_id(&self)->String {wire::user_id(self.0.public_key().as_ref())}
    pub fn identity_proof(&self, challenge: &serde_json::Value, origin: &str, purpose: &str) -> Result<serde_json::Value> {
        ensure!(challenge["origin"] == origin && challenge["purpose"] == purpose && challenge["role"] == "user" && challenge["public_key"] == self.public_key(), "Identity challenge does not match this client");
        let expires=challenge["expires"].as_u64().ok_or_else(||anyhow::anyhow!("Invalid challenge expiry"))?;
        let now=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs();
        ensure!(expires>now && expires<=now+360,"Identity challenge expired");
        let cid=challenge["id"].as_str().ok_or_else(||anyhow::anyhow!("Invalid challenge"))?;
        let nonce=challenge["nonce"].as_str().ok_or_else(||anyhow::anyhow!("Invalid challenge"))?;
        let message=serde_json::to_vec(&("teerouter-identity-v1",origin,purpose,"user",self.public_key(),cid,nonce,expires))?;
        Ok(serde_json::json!({"challenge_id":cid,"signature":wire::hex(self.0.sign(&message).as_ref()),"role":"user"}))
    }
    pub fn enrollment(&self,account: &str,device: &str)->serde_json::Value {
        let public=self.public_key();
        let message=serde_json::to_vec(&("teerouter-user-enrollment-v1",account,device,&public)).unwrap();
        serde_json::json!({"device_id":device,"public_key":public,"proof":wire::hex(self.0.sign(&message).as_ref())})
    }
    pub(crate) fn sign(&self,boot:&str,method:&hyper::Method,uri:&hyper::Uri,headers:&mut hyper::HeaderMap,body:&[u8])->Result<()> {
        ensure!(!boot.is_empty(),"Router signing capability required");
        // The Agent cannot supply its own Router proof or override client identity.
        let remove:Vec<_>=headers.keys().filter(|k|k.as_str().starts_with("x-router-user-")).cloned().collect();
        for name in remove {headers.remove(name);}
        let mut random=[0u8;32];getrandom::getrandom(&mut random).map_err(|_|anyhow::anyhow!("Signing nonce unavailable"))?;
        let nonce=wire::hex(&random);
        let timestamp=std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)?.as_secs().to_string();
        let message=wire::message(boot,&self.user_id(),method,uri,headers,body,&timestamp,&nonce);
        for (name,value) in [("x-router-user-id",self.user_id()),("x-router-user-timestamp",timestamp),("x-router-user-nonce",nonce),("x-router-user-signature",wire::hex(self.0.sign(&message).as_ref()))] {headers.insert(name,value.parse()?);}
        Ok(())
    }
}

#[cfg(test)]
pub(crate) fn fixture()->Signer {Signer::from_seed(&[7;32]).unwrap()}
