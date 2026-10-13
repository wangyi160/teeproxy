//! Local rule configuration. Selection, budget enforcement and timing belong to router.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    #[default]
    Price,
    Speed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "PolicyInput")]
pub struct Policy {
    pub automatic: bool,
    pub max_multiplier_millionths: u64,
    pub priority: Priority,
}
// Accept and discard the removed v0.10 selectors, including in DPAPI desktop profiles.
// They must never survive as invisible routing constraints or be saved again.
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PolicyInput {
    automatic: bool,
    max_multiplier_millionths: u64,
    priority: Priority,
    #[serde(rename = "provider")]
    _provider: Option<serde::de::IgnoredAny>,
    #[serde(rename = "model")]
    _model: Option<serde::de::IgnoredAny>,
}
impl Default for PolicyInput {
    fn default() -> Self {
        Self {
            automatic: false,
            max_multiplier_millionths: 1_000_000,
            priority: Priority::Price,
            _provider: None,
            _model: None,
        }
    }
}
impl From<PolicyInput> for Policy {
    fn from(input: PolicyInput) -> Self {
        Self {
            automatic: input.automatic,
            max_multiplier_millionths: input.max_multiplier_millionths,
            priority: input.priority,
        }
    }
}
impl Default for Policy {
    fn default() -> Self {
        Self {
            automatic: false,
            max_multiplier_millionths: 1_000_000,
            priority: Priority::Price,
        }
    }
}
impl Policy {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.max_multiplier_millionths <= 1_000_000_000_000_000,
            "Invalid maximum multiplier"
        );
        Ok(())
    }
}

/// One request owns one complete rule snapshot, including response validation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Rules {
    pub policy: Policy,
    pub pool: Option<String>,
}
impl Rules {
    pub fn new(policy: Policy, pool: Option<String>) -> Result<Self> {
        policy.validate()?;
        if let Some(pool) = &pool {
            ensure!(
                !pool.is_empty()
                    && pool.len() <= 128
                    && pool
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c)),
                "Invalid pool"
            );
        }
        Ok(Self { policy, pool })
    }
}
pub fn parse_multiplier(text: &str) -> Result<u64> {
    let text = text.trim();
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    ensure!(
        !whole.is_empty()
            && whole.bytes().all(|b| b.is_ascii_digit())
            && fraction.len() <= 6
            && fraction.bytes().all(|b| b.is_ascii_digit()),
        "倍率必须为非负数字，最多 6 位小数"
    );
    let value = whole
        .parse::<u64>()?
        .checked_mul(1_000_000)
        .and_then(|v| {
            v.checked_add(
                fraction.parse::<u64>().unwrap_or(0) * 10u64.pow(6 - fraction.len() as u32),
            )
        })
        .ok_or_else(|| anyhow::anyhow!("倍率过大"))?;
    ensure!(value <= 1_000_000_000_000_000, "倍率过大");
    Ok(value)
}
pub fn format_multiplier(value: u64) -> String {
    format!("{}.{:06}", value / 1_000_000, value % 1_000_000)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

impl Policy {
    pub(crate) fn header(&self, pool: Option<&str>) -> Result<hyper::header::HeaderValue> {
        self.validate()?;
        let mut rule = serde_json::json!({"version":1,"mode":if self.automatic { "auto" } else { "fixed" },
            "max_multiplier_millionths":self.max_multiplier_millionths,"priority":self.priority});
        if !self.automatic {
            rule["pool"] = pool
                .ok_or_else(|| anyhow::anyhow!("routing_pool_required"))?
                .into();
        }
        Ok(serde_json::to_string(&rule)?.parse()?)
    }
}
type Observer = Arc<dyn Fn(String) + Send + Sync>;
#[derive(Default)]
pub(crate) struct State {
    observer: Mutex<Option<Observer>>,
}
impl State {
    pub fn observe(&self, observer: Observer) {
        *self.observer.lock().unwrap() = Some(observer);
    }
    pub(crate) fn notify(&self, text: String) {
        let observer = self.observer.lock().unwrap().clone();
        if let Some(observer) = observer {
            observer(text);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rules_encode_exact_limits_and_migrate_removed_selectors() {
        let policy: Policy = toml::from_str("automatic = true\nprovider = 'claude'\nmodel = 'old-model'\nmax_multiplier_millionths = 250000").unwrap();
        let wire: serde_json::Value =
            serde_json::from_slice(policy.header(Some("ignored")).unwrap().as_bytes()).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"version":1,"mode":"auto","max_multiplier_millionths":250000,"priority":"price"})
        );
        let saved = serde_json::to_value(&policy).unwrap();
        assert!(saved.get("provider").is_none() && saved.get("model").is_none());
        let policy = Policy::default();
        assert!(policy.header(None).is_err());
        let wire: serde_json::Value =
            serde_json::from_slice(policy.header(Some("m_shop.pool")).unwrap().as_bytes()).unwrap();
        assert_eq!(wire["pool"], "m_shop.pool");
        assert_eq!(wire["mode"], "fixed");
        assert!(toml::from_str::<Policy>("max_multipler = 1").is_err());
    }
    #[test]
    fn decimal_limits_are_exact() {
        for (text, value) in [
            ("0", 0),
            ("0.000001", 1),
            ("0.2", 200000),
            ("1", 1000000),
            ("1000000000", 1_000_000_000_000_000),
        ] {
            assert_eq!(parse_multiplier(text).unwrap(), value);
            assert_eq!(parse_multiplier(&format_multiplier(value)).unwrap(), value);
        }
        for text in ["", "-1", "NaN", "0.0000001", "1e6", "1.2.3", "1000000001"] {
            assert!(parse_multiplier(text).is_err());
        }
    }
}
