//! Read-only inventory from the management platform snapshot.
use anyhow::{Context, Result, ensure};
use serde_json::Value;
use std::collections::BTreeSet;

#[derive(Default)]
pub(crate) struct Catalog {
    pub stale: bool,
    pub models: Vec<Model>,
    pub accounts: Vec<Account>,
    pub pools: Vec<Pool>,
}
pub(crate) struct Pool {
    pub id: String,
    pub name: String,
    pub merchant: String,
    pub merchant_id: String,
    pub provider: String,
    pub multiplier: String,
    pub state: String,
    pub label: String,
    pub account_count: usize,
    pub latency: Probe,
}
pub(crate) struct Model {
    pub id: String,
    pub provider: String,
    pub pool: String,
    pub protocols: String,
}
pub(crate) struct Account {
    pub pool: String,
    pub id: String,
    pub provider: String,
    pub state: String,
    pub catalog: String,
    pub models: String,
    pub latency: Probe,
}
pub(crate) struct Probe {
    status: String,
    ms: Option<f64>,
    expires_at: Option<u64>,
    pub checked: String,
}
impl Probe {
    fn parse(value: &Value) -> Self {
        Self {
            status: text(&value["status"]),
            ms: value["latency_ms"]
                .as_f64()
                .filter(|v| v.is_finite() && *v >= 0.0),
            expires_at: value["expires_at"].as_u64(),
            checked: value["checked_at"]
                .as_u64()
                .and_then(checked_time)
                .unwrap_or_else(|| "—".into()),
        }
    }
    fn display_at(&self, now: u64) -> String {
        if self.expires_at.is_some_and(|t| t <= now) {
            return "已过期".into();
        }
        match self.status.as_str() {
            "measured" if self.ms.is_some() => format!("{:.1} ms", self.ms.unwrap()),
            "failed" => "测速失败".into(),
            "stale" => "已过期".into(),
            "unavailable" => "不可用".into(),
            _ => "未测速".into(),
        }
    }
    pub fn display(&self) -> String {
        self.display_at(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        )
    }
}
pub(super) fn checked_time(seconds: u64) -> Option<String> {
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::{FileTimeToSystemTime, SystemTimeToTzSpecificLocalTime},
    };
    let ticks = seconds
        .checked_mul(10_000_000)?
        .checked_add(116_444_736_000_000_000)?;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let (mut utc, mut local) = (SYSTEMTIME::default(), SYSTEMTIME::default());
    if unsafe { FileTimeToSystemTime(&file, &mut utc) } == 0
        || unsafe { SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) } == 0
    {
        return None;
    }
    Some(format!(
        "{:02}-{:02} {:02}:{:02}:{:02}",
        local.wMonth, local.wDay, local.wHour, local.wMinute, local.wSecond
    ))
}
fn text(value: &Value) -> String {
    value
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect()
}
fn joined(values: impl Iterator<Item = String>) -> String {
    values
        .filter(|s| !s.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(" / ")
}
fn provider(value: &Value) -> String {
    match value.as_str().unwrap_or("") {
        "openai" | "chatgpt" => "ChatGPT".into(),
        "anthropic" | "claude" => "Claude".into(),
        "xai" | "grok" => "Grok".into(),
        "workbuddy" => "Workbuddy（测试）".into(),
        _ => text(value),
    }
}
fn model_namespace(value: &Value) -> Option<&'static str> {
    match value.as_str()? {
        "openai" | "chatgpt" => Some("chatgpt"),
        "anthropic" | "claude" => Some("claude"),
        "xai" | "grok" => Some("grok"),
        "workbuddy" => Some("workbuddy"),
        _ => None,
    }
}
fn multiplier(value: &Value) -> String {
    let Some(n) = value.as_u64().filter(|n| *n <= 1_000_000_000_000_000) else {
        return "未设置".into();
    };
    let whole = n / 1_000_000;
    let fraction = n % 1_000_000;
    if fraction == 0 {
        format!("{whole}×")
    } else {
        format!(
            "{whole}.{}×",
            format!("{fraction:06}").trim_end_matches('0')
        )
    }
}
pub(crate) fn parse(models: &Value, pools: &Value) -> Result<Catalog> {
    let models = models["data"]
        .as_array()
        .context("模型目录缺少 data 列表")?;
    let pools = pools["data"]
        .as_array()
        .context("账号池目录缺少 data 列表")?;
    ensure!(
        models.len() <= 10000 && pools.len() <= 10000,
        "目录记录过多"
    );
    let mut result = Catalog::default();
    for pool in pools {
        let name = text(&pool["id"]);
        let pool_name = match text(&pool["pool_name"]) {
            s if s.is_empty() => name.clone(),
            s => s,
        };
        let merchant = text(&pool["merchant_name"]);
        let rate = multiplier(&pool["multiplier_millionths"]);
        ensure!(!name.is_empty(), "账号池目录缺少 ID");
        let accounts = pool["accounts"]
            .as_array()
            .context("账号池缺少 accounts 列表")?;
        ensure!(
            result.accounts.len() + accounts.len() <= 10000,
            "账号记录过多"
        );
        ensure!(!result.pools.iter().any(|p| p.id == name), "账号池 ID 重复");
        // The user directory contains only accounts Router reports as usable.
        // Probe failures and occupied concurrency do not override this flag.
        let accounts: Vec<_> = accounts
            .iter()
            .filter(|a| a["enabled"] == true && a["available"] == true)
            .collect();
        if accounts.is_empty() {
            continue;
        }
        let label = [pool_name.as_str(), merchant.as_str(), rate.as_str()]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · ");
        result.pools.push(Pool {
            id: name.clone(),
            name: pool_name.clone(),
            merchant: merchant.clone(),
            merchant_id: text(&pool["merchant_id"]),
            provider: provider(&pool["provider"]),
            multiplier: rate.clone(),
            state: "可用".into(),
            label,
            account_count: accounts.len(),
            latency: Probe::parse(&pool["proxy_latency"]),
        });
        for account in accounts {
            ensure!(result.accounts.len() < 10000, "账号记录过多");
            let id = text(&account["id"]);
            ensure!(!id.is_empty(), "账号目录缺少 ID");
            let catalog = match account["catalog"]["state"].as_str() {
                Some("fetched") => "已获取",
                Some("refreshing") => "刷新中",
                Some("not_fetched") => "尚未获取",
                Some("error") => "获取失败",
                Some("disabled") => "已停用",
                _ => "未知",
            };
            result.accounts.push(Account {
                pool: name.clone(),
                id,
                provider: provider(&account["provider"]),
                state: "可用".into(),
                catalog: catalog.into(),
                models: account["catalog"]["visible_models"]
                    .as_u64()
                    .map(|v| v.to_string())
                    .unwrap_or("—".into()),
                latency: Probe::parse(&account["proxy_latency"]),
            });
        }
    }
    // A row is a (pool, model) pair. Never merge prices or capabilities across pools.
    let mut seen = BTreeSet::new();
    for model in models {
        let wire_id = text(&model["id"]);
        ensure!(!wire_id.is_empty(), "模型目录缺少模型 ID");
        let memberships: BTreeSet<_> = model["router_pools"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        for membership in &memberships {
            // The pool endpoint is authoritative for the pools this key may see.
            let Some(pool) = result.pools.iter().find(|p| p.id == *membership) else {
                continue;
            };
            let id = if let Some(namespace) = model_namespace(&model["router_provider"]) {
                let raw = model["router_model"]
                    .as_str()
                    .context("Missing official model ID")?;
                ensure!(
                    !raw.is_empty() && wire_id == format!("{namespace}/{raw}"),
                    "Model namespace does not match its provider"
                );
                ensure!(
                    pool.provider.is_empty()
                        || pool.provider == provider(&model["router_provider"]),
                    "Model provider does not match its pool"
                );
                ensure!(
                    pool.merchant_id.is_empty()
                        || model["router_merchants"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .any(|m| {
                                m["id"] == pool.merchant_id
                                    && m["pools"]
                                        .as_array()
                                        .into_iter()
                                        .flatten()
                                        .any(|p| p == membership)
                            }),
                    "Model merchant does not own its pool"
                );
                wire_id.clone()
            } else {
                // Older routers still accept bare IDs with the routing rule.
                let merchant = model["router_merchant"]["id"].as_str().unwrap_or("");
                ensure!(
                    pool.merchant_id.is_empty() || merchant == pool.merchant_id,
                    "模型与账号池的商户不匹配，请刷新列表"
                );
                if merchant.is_empty() {
                    wire_id.clone()
                } else {
                    wire_id
                        .strip_prefix(&format!("{merchant}/"))
                        .filter(|s| !s.is_empty())
                        .context("模型 ID 与商户不匹配")?
                        .to_owned()
                }
            };
            if !seen.insert((pool.id.clone(), id.clone())) {
                continue;
            }
            ensure!(result.models.len() < 10000, "账号池模型记录过多");
            let caps: Vec<_> = model["router_capabilities"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| {
                    c["pool"].as_str() == Some(membership)
                        || (memberships.len() == 1 && c["pool"].is_null())
                })
                .collect();
            let source = if pool.provider.is_empty() {
                joined(caps.iter().map(|c| provider(&c["provider"])))
            } else {
                pool.provider.clone()
            };
            let protocols = joined(
                caps.iter()
                    .flat_map(|c| c["protocols"].as_array().into_iter().flatten())
                    .map(text),
            );
            result.models.push(Model {
                id,
                pool: pool.id.clone(),
                provider: source,
                protocols,
            });
        }
    }
    result
        .pools
        .sort_by(|a, b| (&a.name, &a.merchant, &a.id).cmp(&(&b.name, &b.merchant, &b.id)));
    let mut labels = std::collections::BTreeMap::<String, usize>::new();
    for pool in &result.pools {
        *labels.entry(pool.label.clone()).or_default() += 1;
    }
    for pool in &mut result.pools {
        if labels[&pool.label] > 1 {
            pool.label = format!("{} · {}", pool.label, pool.id);
        }
    }
    result
        .models
        .sort_by(|a, b| (&a.pool, &a.id).cmp(&(&b.pool, &b.id)));
    result
        .accounts
        .sort_by(|a, b| (&a.pool, &a.id).cmp(&(&b.pool, &b.id)));
    Ok(result)
}
#[cfg(test)]
mod tests {
    #[test]
    fn latency_display_handles_missing_failed_and_expired_measurements() {
        use super::*;
        let value = serde_json::json!({"status":"measured", "latency_ms":170.25,
            "checked_at":1792000000, "expires_at":1792000600, "proxy":"must-never-display"});
        let probe = Probe::parse(&value);
        assert_eq!(probe.display_at(1792000000), "170.2 ms");
        assert_eq!(probe.display_at(1792000600), "已过期");
        assert_ne!(probe.checked, "—");
        assert_eq!(Probe::parse(&Value::Null).display_at(0), "未测速");
        assert_eq!(
            Probe::parse(&serde_json::json!({"status":"failed","latency_ms":1})).display_at(0),
            "测速失败"
        );
        assert_eq!(
            Probe::parse(&serde_json::json!({"status":"measured","latency_ms":-1})).display_at(0),
            "未测速"
        );
    }
    use super::*;
    use serde_json::json;
    #[test]
    fn provider_namespaced_models_preserve_identity_across_merchant_pools() {
        let pools = json!({"data":[
            {"id":"m_a.first","merchant_id":"m_a","provider":"openai","accounts":[{"id":"ready","enabled":true,"available":true}]},
            {"id":"m_b.second","merchant_id":"m_b","provider":"openai","accounts":[{"id":"ready","enabled":true,"available":true}]},
            {"id":"m_b.grok","merchant_id":"m_b","provider":"xai","accounts":[{"id":"ready","enabled":true,"available":true}]}
        ]});
        let models = json!({"data":[
            {"id":"chatgpt/shared-model","router_provider":"openai","router_model":"shared-model",
                "router_pools":["m_a.first","m_b.second"],
                "router_merchants":[{"id":"m_a","pools":["m_a.first"]},{"id":"m_b","pools":["m_b.second"]}]},
            {"id":"grok/shared-model","router_provider":"xai","router_model":"shared-model",
                "router_pools":["m_b.grok"],"router_merchants":[{"id":"m_b","pools":["m_b.grok"]}]}
        ]});
        let catalog = parse(&models, &pools).unwrap();
        assert_eq!(catalog.models.len(), 3);
        assert_eq!(
            catalog
                .models
                .iter()
                .filter(|m| m.id == "chatgpt/shared-model")
                .count(),
            2
        );
        assert_eq!(
            catalog
                .models
                .iter()
                .filter(|m| m.id == "grok/shared-model")
                .count(),
            1
        );
        assert_eq!(catalog.models[0].provider, "ChatGPT");
        let mut bad = models.clone();
        bad["data"][0]["id"] = json!("grok/shared-model");
        assert!(parse(&bad, &pools).is_err());
        let mut bad = models.clone();
        bad["data"][0]["router_merchants"][0]["pools"] = json!(["m_b.second"]);
        assert!(parse(&bad, &pools).is_err());
        let mut bad = pools.clone();
        bad["data"][0]["provider"] = json!("xai");
        assert!(parse(&models, &bad).is_err());
    }
    #[test]
    fn models_are_scoped_to_authorized_pools_with_distinct_merchants_and_capabilities() {
        let pools = json!({"data":[
            {"id":"m_a.cheap","pool_name":"cheap","merchant_id":"m_a","merchant_name":"Shop","provider":"openai","multiplier_millionths":200000,"accounts":[{"id":"ready","enabled":true,"available":true}]},
            {"id":"m_a.premium","pool_name":"premium","merchant_id":"m_a","merchant_name":"Shop","provider":"openai","multiplier_millionths":1000000,"accounts":[{"id":"ready","enabled":true,"available":true}]},
            {"id":"m_b.cheap","pool_name":"cheap","merchant_id":"m_b","merchant_name":"Shop","provider":"openai","multiplier_millionths":200000,"accounts":[{"id":"ready","enabled":true,"available":true}]}
        ]});
        let models = json!({"data":[
            {"id":"m_a/vendor/model","router_merchant":{"id":"m_a"},"router_pools":["m_a.cheap","m_a.premium","m_a.hidden"],"router_capabilities":[
                {"pool":"m_a.cheap","protocols":["responses"]},
                {"pool":"m_a.premium","protocols":["chat_completions"]},
                {"pool":"m_a.hidden","protocols":["never-expose"]}]},
            {"id":"m_b/vendor/model","router_merchant":{"id":"m_b"},"router_pools":["m_b.cheap"]}
        ]});
        let catalog = parse(&models, &pools).unwrap();
        assert_eq!(catalog.models.len(), 3);
        assert!(catalog.models.iter().all(|m| m.id == "vendor/model"));
        assert_eq!(catalog.models[0].pool, "m_a.cheap");
        assert_eq!(catalog.models[0].protocols, "responses");
        assert_eq!(catalog.models[1].protocols, "chat_completions");
        assert!(catalog.models[2].protocols.is_empty());
        assert_eq!(catalog.pools[0].multiplier, "0.2×");
        assert_ne!(catalog.pools[0].label, catalog.pools[1].label);
        let mut wrong = models.clone();
        wrong["data"][0]["router_merchant"]["id"] = json!("m_b");
        assert!(parse(&wrong, &pools).is_err());
        let mut duplicate = models.clone();
        duplicate["data"]
            .as_array_mut()
            .unwrap()
            .push(models["data"][0].clone());
        assert_eq!(parse(&duplicate, &pools).unwrap().models.len(), 3);
    }
    #[test]
    fn pool_multiplier_is_exact_and_missing_is_not_assumed_to_be_one() {
        for (value, expected) in [
            (json!(20000), "0.02×"),
            (json!(200000), "0.2×"),
            (json!(1000000), "1×"),
            (json!(1), "0.000001×"),
            (json!(0), "0×"),
            (json!(1000000000000000u64), "1000000000×"),
            (Value::Null, "未设置"),
            (json!(-1), "未设置"),
            (json!(0.2), "未设置"),
        ] {
            assert_eq!(multiplier(&value), expected);
        }
        let catalog = parse(&json!({"data":[]}), &json!({"data":[{"id":"m_id.pool","pool_name":"pool","merchant_name":"Shop","multiplier_millionths":200000,"accounts":[{"id":"ready","enabled":true,"available":true}]}]})).unwrap();
        let row = &catalog.pools[0];
        assert_eq!(row.id, "m_id.pool");
        assert_eq!(row.name, "pool");
        assert_eq!(row.merchant, "Shop");
        assert_eq!(row.multiplier, "0.2×");
        assert_eq!(row.account_count, 1);
        assert_eq!(catalog.accounts.len(), 1);
    }
    #[test]
    fn inventory_hides_unusable_accounts_empty_pools_and_their_models() {
        let models = json!({"data":[
            {"id":"model-a","router_pools":["p","bad","empty"],"router_capabilities":[{"pool":"p","provider":"openai","protocols":["responses"]}]},
            {"id":"model-b","router_pools":["bad"]}
        ]});
        let mut pools = json!({"data":[
            {"id":"p","account_count":99,"accounts":[
                {"id":"ready","provider":"openai","enabled":true,"available":true,"catalog":{"state":"fetched","visible_models":1},"proxy_latency":{"status":"failed"}},
                {"id":"disabled","enabled":false,"available":true},
                {"id":"expired","enabled":true,"available":false,"credential":"NEVER_DISPLAY"},
                {"id":"unknown","enabled":true}
            ]},
            {"id":"bad","accounts":[{"id":"unavailable","enabled":true,"available":false}]},
            {"id":"empty","accounts":[]}
        ]});
        let catalog = parse(&models, &pools).unwrap();
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].id, "model-a");
        assert_eq!(catalog.models[0].protocols, "responses");
        assert_eq!(catalog.accounts.len(), 1);
        assert_eq!(catalog.accounts[0].id, "ready");
        assert_eq!(catalog.accounts[0].state, "可用");
        assert_eq!(catalog.accounts[0].latency.display_at(0), "测速失败");
        assert_eq!(catalog.pools.len(), 1);
        assert_eq!(catalog.pools[0].id, "p");
        assert_eq!(catalog.pools[0].account_count, 1);
        pools["data"][0]["accounts"][0]["available"] = json!(false);
        let hidden = parse(&models, &pools).unwrap();
        assert!(hidden.pools.is_empty());
        assert!(hidden.accounts.is_empty());
        assert!(hidden.models.is_empty());
        pools["data"][0]["accounts"][0]["available"] = json!(true);
        assert_eq!(parse(&models, &pools).unwrap().pools.len(), 1);
        assert!(parse(&json!({"data":[]}), &json!({"error":"bad"})).is_err());
    }
}
