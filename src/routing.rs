//! Local routing preferences and passive measurements; never stores prompts or responses.
use crate::{client::VerifiedSession, protocol::MAX_BODY};
use anyhow::{Result, ensure};
use bytes::Bytes;
use http_body_util::{BodyExt, Full, Limited};
use hyper::{Request, header::HeaderValue};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    #[default]
    Price,
    Speed,
}

#[derive(Clone, Serialize, Deserialize)]
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
fn provider(s: &str) -> &str {
    match s {
        "anthropic" => "claude",
        "xai" => "grok",
        _ => s,
    }
}
pub(crate) fn protocol(path: &str) -> Option<&'static str> {
    match path {
        "/v1/chat/completions" => Some("chat_completions"),
        "/v1/responses" => Some("responses"),
        "/v1/messages" => Some("messages"),
        _ => None,
    }
}

#[derive(Clone)]
pub(crate) struct Candidate {
    pub pool: String,
    pub label: String,
    pub model: String,
    pub merchant: String,
    pub provider: String,
    pub protocol: String,
    pub multiplier: u64,
    pub version: u64,
}
impl Candidate {
    fn key(&self) -> String {
        format!("{}\n{}\n{}", self.pool, self.model, self.protocol)
    }
}

pub(crate) fn candidates(models: &Value, pools: &Value) -> Result<Vec<Candidate>> {
    let models = models["data"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid model inventory"))?;
    let pools = pools["data"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Invalid pool inventory"))?;
    ensure!(
        models.len() <= 10000 && pools.len() <= 10000,
        "Inventory too large"
    );
    let mut result = Vec::new();
    for pool in pools {
        let (Some(id), Some(merchant), Some(rate), Some(version)) = (
            pool["id"].as_str(),
            pool["merchant_id"].as_str(),
            pool["multiplier_millionths"].as_u64(),
            pool["price_version"].as_u64(),
        ) else {
            continue;
        };
        if !id.starts_with(&format!("{merchant}."))
            || id.len() > 128
            || !id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
            || rate > 1_000_000_000_000_000
        {
            continue;
        }
        if !pool["accounts"].as_array().is_some_and(|a| {
            a.iter()
                .any(|a| a["enabled"] == true && a["available"] == true)
        }) {
            continue;
        }
        for model in models {
            if model["router_merchant"]["id"] != merchant
                || model["router_pricing"]["version"].as_u64() != Some(version)
                || !model["router_pools"]
                    .as_array()
                    .is_some_and(|p| p.iter().any(|p| p == id))
            {
                continue;
            }
            let Some(name) = model["id"]
                .as_str()
                .and_then(|m| m.strip_prefix(&format!("{merchant}/")))
                .filter(|s| !s.is_empty() && s.len() <= 512 && !s.chars().any(char::is_control))
            else {
                continue;
            };
            // Require an actual, version-consistent quote for this model and pool.
            if !model["router_pricing"]["rates"]
                .as_array()
                .is_some_and(|rates| {
                    rates.iter().any(|q| {
                        q["pool"] == pool["pool_name"]
                            && q["model"] == name
                            && q["multiplier_millionths"].as_u64() == Some(rate)
                    })
                })
            {
                continue;
            }
            for cap in model["router_capabilities"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|c| c["pool"] == id)
            {
                let source = provider(cap["provider"].as_str().unwrap_or(""));
                for protocol in cap["protocols"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                {
                    if !["chat_completions", "responses", "messages"].contains(&protocol) {
                        continue;
                    }
                    if result.iter().any(|c: &Candidate| {
                        c.pool == id && c.model == name && c.protocol == protocol
                    }) {
                        continue;
                    }
                    ensure!(result.len() < 30000, "Inventory too large");
                    let label = format!(
                        "{} · {}",
                        pool["pool_name"].as_str().unwrap_or(id),
                        pool["merchant_name"].as_str().unwrap_or(merchant)
                    )
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(180)
                    .collect();
                    result.push(Candidate {
                        pool: id.into(),
                        merchant: merchant.into(),
                        label,
                        model: name.into(),
                        provider: source.into(),
                        protocol: protocol.into(),
                        multiplier: rate,
                        version,
                    });
                }
            }
        }
    }
    Ok(result)
}

#[derive(Default)]
struct Metric {
    first: Option<f64>,
    tokens_per_sec: Option<f64>,
    sampled: Option<Instant>,
    cooldown: Option<Instant>,
    in_flight: usize,
}
type Observer = Arc<dyn Fn(String) + Send + Sync>;
#[derive(Default)]
pub(crate) struct State {
    history: Mutex<HashMap<String, Metric>>,
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
    pub async fn inventory(
        &self,
        session: &mut VerifiedSession,
        auth: &HeaderValue,
    ) -> Result<Vec<Candidate>> {
        // Read a fresh quote before every generation. The router checks its version
        // again at admission, closing the race between this read and forwarding.
        let mut values = Vec::new();
        for path in ["/v1/models", "/v1/pools"] {
            let mut request = Request::new(Full::new(Bytes::new()));
            *request.uri_mut() = path.parse()?;
            request.headers_mut().insert("authorization", auth.clone());
            let response = session.send_request(request).await?;
            ensure!(
                response.status().is_success(),
                "Routing inventory unavailable"
            );
            let bytes = tokio::time::timeout(
                Duration::from_secs(30),
                Limited::new(response.into_body(), MAX_BODY).collect(),
            )
            .await?
            .map_err(|_| anyhow::anyhow!("Routing inventory response invalid or too large"))?
            .to_bytes();
            values.push(serde_json::from_slice(&bytes)?);
        }
        candidates(&values[0], &values[1])
    }
    pub fn choose(
        self: &Arc<Self>,
        rows: &[Candidate],
        policy: &Policy,
        model: &str,
        protocol: &str,
        fixed_pool: Option<&str>,
    ) -> Result<Observation, &'static str> {
        let (merchant, model) = model
            .split_once('/')
            .filter(|(m, _)| m.starts_with("m_"))
            .map_or((None, model), |(m, id)| (Some(m), id));
        let mut eligible = rows
            .iter()
            .filter(|c| {
                c.model == model
                    && c.protocol == protocol
                    && merchant.is_none_or(|m| m == c.merchant)
                    && fixed_pool.is_none_or(|pool| c.pool == pool)
                    && ["openai", "claude", "grok", "workbuddy"].contains(&c.provider.as_str())
            })
            .collect::<Vec<_>>();
        if fixed_pool.is_some()
            && !eligible.is_empty()
            && eligible
                .iter()
                .all(|c| c.multiplier > policy.max_multiplier_millionths)
        {
            return Err("routing_multiplier_exceeded");
        }
        eligible.retain(|c| c.multiplier <= policy.max_multiplier_millionths);
        let mut history = self.history.lock().unwrap();
        // Bound memory to entries still present in the authenticated inventory.
        if history.len() > 30000 {
            history.retain(|key, _| rows.iter().any(|c| c.key() == *key));
        }
        let now = Instant::now();
        eligible.retain(|c| {
            fixed_pool.is_some()
                || !history
                    .get(&c.key())
                    .is_some_and(|m| m.cooldown.is_some_and(|until| until > now))
        });
        let metric = |c: &Candidate| history.get(&c.key());
        let fresh = |c: &Candidate| {
            metric(c)
                .filter(|m| {
                    m.sampled
                        .is_some_and(|t| now.duration_since(t) < Duration::from_secs(600))
                })
                .and_then(|m| m.first)
        };
        eligible.sort_by(|a, b| {
            let price = a.multiplier.cmp(&b.multiplier);
            if policy.priority == Priority::Price {
                return price.then(a.pool.cmp(&b.pool));
            }
            // Unknown/stale pools receive actual requests, one concurrent sample per pool.
            let rank = |c: &Candidate| {
                if fresh(c).is_none() {
                    if metric(c).is_some_and(|m| m.in_flight > 0) {
                        2
                    } else {
                        0
                    }
                } else {
                    1
                }
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| match (fresh(a), fresh(b)) {
                    (Some(a_first), Some(b_first)) => {
                        // Fixed latency bands keep ordering transitive; within 100 ms compare TPS.
                        (a_first * 10.0)
                            .floor()
                            .total_cmp(&(b_first * 10.0).floor())
                            .then_with(|| {
                                metric(b)
                                    .and_then(|m| m.tokens_per_sec)
                                    .unwrap_or(0.0)
                                    .total_cmp(
                                        &metric(a).and_then(|m| m.tokens_per_sec).unwrap_or(0.0),
                                    )
                            })
                    }
                    _ => metric(a)
                        .and_then(|m| m.sampled)
                        .cmp(&metric(b).and_then(|m| m.sampled)),
                })
                .then(price)
                .then(a.pool.cmp(&b.pool))
        });
        let candidate = (*eligible.first().ok_or("routing_no_matching_pool")?).clone();
        let entry = history.entry(candidate.key()).or_default();
        entry.in_flight += 1;
        let sampled = entry.sampled.is_some();
        drop(history);
        self.notify(format!(
            "{} → {} · {} · {}×{}",
            if fixed_pool.is_some() {
                "指定账号池"
            } else {
                "自动匹配"
            },
            candidate.label,
            candidate.model,
            format_multiplier(candidate.multiplier),
            if fixed_pool.is_none() && policy.priority == Priority::Speed && !sampled {
                " · 积累测速样本"
            } else {
                ""
            }
        ));
        Ok(Observation {
            state: self.clone(),
            candidate,
            started: Instant::now(),
            first: None,
            last: None,
            tokens: None,
            buffer: Vec::new(),
            event: Vec::new(),
            stream: false,
            terminal: false,
            error: false,
            overflow: false,
            finished: false,
        })
    }
}

pub(crate) struct Observation {
    state: Arc<State>,
    pub candidate: Candidate,
    started: Instant,
    first: Option<f64>,
    last: Option<f64>,
    tokens: Option<u64>,
    buffer: Vec<u8>,
    event: Vec<u8>,
    stream: bool,
    terminal: bool,
    error: bool,
    overflow: bool,
    finished: bool,
}
impl Observation {
    pub fn response(&mut self, status: u16, streaming: bool) {
        self.error = !(200..300).contains(&status);
        self.stream = streaming;
    }
    pub fn feed(&mut self, bytes: &[u8]) {
        if !self.stream || self.error || self.overflow {
            return;
        }
        if self.buffer.len() + bytes.len() > MAX_BODY {
            self.overflow = true;
            self.buffer.clear();
            return;
        }
        self.buffer.extend_from_slice(bytes);
        while let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<_> = self.buffer.drain(..=end).collect();
            let line = line.strip_suffix(b"\n").unwrap_or(&line);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                let event = std::mem::take(&mut self.event);
                if event == b"[DONE]" {
                    self.terminal = true;
                } else if let Ok(value) = serde_json::from_slice(&event) {
                    self.value(&value);
                }
            } else if let Some(data) = line.strip_prefix(b"data:") {
                let data = data.strip_prefix(b" ").unwrap_or(data);
                if self.event.len() + data.len() + 1 > MAX_BODY {
                    self.overflow = true;
                    self.event.clear();
                    return;
                }
                if !self.event.is_empty() {
                    self.event.push(b'\n');
                }
                self.event.extend_from_slice(data);
            }
        }
    }
    fn value(&mut self, value: &Value) {
        if value.get("error").is_some_and(|v| !v.is_null())
            || matches!(
                value["type"].as_str(),
                Some("error" | "response.failed" | "response.incomplete")
            )
        {
            self.error = true;
        }
        let nonempty = |v: &Value| v.as_str().is_some_and(|s| !s.is_empty());
        let content = nonempty(&value["delta"])
            || ["text", "thinking", "partial_json"]
                .iter()
                .any(|key| nonempty(&value["delta"][key]))
            || value["choices"].as_array().is_some_and(|choices| {
                choices.iter().any(|c| {
                    ["content", "reasoning_content", "refusal"]
                        .iter()
                        .any(|k| nonempty(&c["delta"][k]))
                        || c["delta"]["tool_calls"].as_array().is_some_and(|t| {
                            t.iter().any(|t| nonempty(&t["function"]["arguments"]))
                        })
                })
            });
        if content {
            let at = self.started.elapsed().as_secs_f64();
            self.first.get_or_insert(at);
            self.last = Some(at);
        }
        for pointer in [
            "/usage/completion_tokens",
            "/usage/output_tokens",
            "/response/usage/output_tokens",
        ] {
            if let Some(tokens) = value.pointer(pointer).and_then(Value::as_u64) {
                self.tokens = Some(tokens);
            }
        }
        if matches!(
            value["type"].as_str(),
            Some("message_stop" | "response.completed")
        ) {
            self.terminal = true;
        }
    }
    pub fn finish(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        // Non-stream responses cannot supply a true time-to-first-token or generation rate.
        if !self.stream || !self.terminal || self.error || self.overflow {
            let mut history = self.state.history.lock().unwrap();
            let metric = history.entry(self.candidate.key()).or_default();
            if self.error || (self.stream && !self.terminal) {
                metric.cooldown = Some(Instant::now() + Duration::from_secs(30));
            } else {
                metric.sampled = Some(Instant::now());
            }
            drop(history);
            if self.error {
                self.state.notify(format!(
                    "上次匹配 {} 返回错误；下次请求重新评估",
                    self.candidate.label
                ));
            }
            return;
        }
        if let Some(first) = self.first {
            let speed = self
                .last
                // Coalesced/buffered bursts do not reveal the upstream's generation rate.
                .filter(|last| *last >= first + 0.25)
                .zip(self.tokens.filter(|n| *n > 1))
                .map(|(last, tokens)| (tokens - 1) as f64 / (last - first));
            let mut history = self.state.history.lock().unwrap();
            let metric = history.entry(self.candidate.key()).or_default();
            metric.first = Some(metric.first.map_or(first, |old| old * 0.7 + first * 0.3));
            if let Some(speed) = speed {
                metric.tokens_per_sec = Some(
                    metric
                        .tokens_per_sec
                        .map_or(speed, |old| old * 0.7 + speed * 0.3),
                );
            }
            metric.sampled = Some(Instant::now());
            metric.cooldown = None;
            drop(history);
            self.state.notify(format!(
                "已使用 {} · {} · 首字 {:.0} ms{}",
                self.candidate.label,
                self.candidate.model,
                first * 1000.0,
                speed
                    .map(|v| format!(" · {:.1} token/s", v))
                    .unwrap_or_default()
            ));
        } else {
            self.state
                .history
                .lock()
                .unwrap()
                .entry(self.candidate.key())
                .or_default()
                .sampled = Some(Instant::now());
        }
    }
    pub fn fail(&mut self) {
        self.error = true;
        self.finish();
    }
}
impl Drop for Observation {
    fn drop(&mut self) {
        if let Some(metric) = self
            .state
            .history
            .lock()
            .unwrap()
            .get_mut(&self.candidate.key())
        {
            metric.in_flight = metric.in_flight.saturating_sub(1);
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;
    pub(crate) fn fixture() -> (Value, Value) {
        let mut models = Vec::new();
        let mut pools = Vec::new();
        for (merchant, rate) in [("m_a", 200000), ("m_b", 500000), ("m_c", 2000000)] {
            pools.push(json!({"id":format!("{merchant}.pool"),"pool_name":"pool","merchant_id":merchant,"merchant_name":merchant,"price_version":7,"multiplier_millionths":rate,"accounts":[{"id":"a","enabled":true,"available":true}]}));
            models.push(json!({"id":format!("{merchant}/model-a"),"router_merchant":{"id":merchant},"router_pools":[format!("{merchant}.pool")],"router_pricing":{"version":7,"rates":[{"pool":"pool","model":"model-a","multiplier_millionths":rate}]},"router_capabilities":[{"pool":format!("{merchant}.pool"),"provider":"openai","protocols":["responses","chat_completions"]}]}));
        }
        (json!({"data":models}), json!({"data":pools}))
    }
    #[test]
    fn decimal_limit_is_exact_and_validated() {
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
        for invalid in ["", "-1", "NaN", "0.0000001", "1e6", "1.2.3", "1000000001"] {
            assert!(parse_multiplier(invalid).is_err());
        }
    }
    #[test]
    fn auto_candidates_require_available_accounts_matching_quotes_and_protocols() {
        let (mut models, mut pools) = fixture();
        assert_eq!(candidates(&models, &pools).unwrap().len(), 6);
        pools["data"][0]["accounts"][0]["enabled"] = json!(false);
        models["data"][1]["router_pricing"]["version"] = json!(6);
        models["data"][2]["router_pricing"]["rates"][0]["multiplier_millionths"] = json!(1);
        assert!(candidates(&models, &pools).unwrap().is_empty());
    }
    #[test]
    fn matching_is_strict_and_price_never_exceeds_the_limit() {
        let (models, pools) = fixture();
        let rows = candidates(&models, &pools).unwrap();
        let state = Arc::new(State::default());
        let mut policy = Policy {
            automatic: true,
            ..Default::default()
        };
        assert_eq!(
            state
                .choose(&rows, &policy, "model-a", "responses", None)
                .unwrap()
                .candidate
                .pool,
            "m_a.pool"
        );
        assert_eq!(
            state
                .choose(&rows, &policy, "m_b/model-a", "responses", None)
                .unwrap()
                .candidate
                .pool,
            "m_b.pool"
        );
        assert!(
            state
                .choose(&rows, &policy, "m_c/model-a", "responses", None)
                .is_err()
        );
        assert!(
            state
                .choose(&rows, &policy, "model-a", "messages", None)
                .is_err()
        );
        policy.max_multiplier_millionths = 199999;
        assert!(
            state
                .choose(&rows, &policy, "model-a", "responses", None)
                .is_err()
        );
        policy.max_multiplier_millionths = 1000000;
        assert_eq!(
            state
                .choose(&rows, &policy, "model-a", "responses", Some("m_b.pool"))
                .unwrap()
                .candidate
                .pool,
            "m_b.pool"
        );
        policy.max_multiplier_millionths = 499999;
        assert_eq!(
            state
                .choose(&rows, &policy, "model-a", "responses", Some("m_b.pool"))
                .err(),
            Some("routing_multiplier_exceeded")
        );
        policy.max_multiplier_millionths = 500000;
        assert!(
            state
                .choose(&rows, &policy, "model-a", "responses", Some("m_b.pool"))
                .is_ok()
        );
        assert!(
            state
                .choose(&rows, &policy, "m_a/model-a", "responses", Some("m_b.pool"))
                .is_err()
        );
        let mut test_rows = rows;
        for r in &mut test_rows {
            r.provider = "workbuddy".into();
        }
        assert!(
            state
                .choose(&test_rows, &policy, "model-a", "responses", None)
                .is_ok()
        );
    }
    #[test]
    fn old_selectors_are_ignored_and_not_saved_but_the_limit_is_preserved() {
        let policy: Policy = serde_json::from_value(json!({"automatic":true,"provider":"claude","model":"old-model","max_multiplier_millionths":250000,"priority":"speed"})).unwrap();
        assert!(policy.automatic && policy.priority == Priority::Speed);
        assert_eq!(policy.max_multiplier_millionths, 250000);
        let saved = serde_json::to_value(&policy).unwrap();
        assert!(saved.get("model").is_none() && saved.get("provider").is_none());
        let (models, pools) = fixture();
        assert!(
            Arc::new(State::default())
                .choose(
                    &candidates(&models, &pools).unwrap(),
                    &policy,
                    "model-a",
                    "responses",
                    None
                )
                .is_ok()
        );
        let policy: Policy = toml::from_str("automatic = false\nprovider = 'grok'\nmodel = 'old'\nmax_multiplier_millionths = 500000").unwrap();
        assert!(!policy.automatic);
        assert_eq!(policy.max_multiplier_millionths, 500000);
        assert!(toml::from_str::<Policy>("max_multipler = 1").is_err());
    }
    #[test]
    fn speed_uses_real_content_not_headers_and_explores_unmeasured_pools() {
        let (models, pools) = fixture();
        let rows = candidates(&models, &pools).unwrap();
        let state = Arc::new(State::default());
        let policy = Policy {
            automatic: true,
            priority: Priority::Speed,
            ..Default::default()
        };
        let mut first = state
            .choose(&rows, &policy, "model-a", "responses", None)
            .unwrap();
        let second = state
            .choose(&rows, &policy, "model-a", "responses", None)
            .unwrap();
        assert_ne!(first.candidate.pool, second.candidate.pool);
        first.response(200, true);
        first.feed(b": keepalive\n\ndata: {\"type\":\"response.created\"}\n\n");
        assert!(first.first.is_none());
        first.started = Instant::now() - Duration::from_secs(2);
        first.feed(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"a\"}\n\n");
        assert!(first.first.unwrap() >= 2.0);
        first.started -= Duration::from_secs(1);
        first.feed(b"data: {\"type\":\"response.output_text.delta\",\"delta\":\"b\"}\n\ndata: {\"type\":\"response.completed\",\"response\":{\"usage\":{\"output_tokens\":11}}}\n\n");
        first.finish();
        let a = first.candidate.key();
        let b = second.candidate.key();
        drop(first);
        drop(second);
        assert!(state.history.lock().unwrap()[&a].tokens_per_sec.unwrap() > 9.0);
        state.history.lock().unwrap().insert(
            b.clone(),
            Metric {
                first: Some(0.5),
                tokens_per_sec: Some(20.0),
                sampled: Some(Instant::now()),
                ..Default::default()
            },
        );
        let chosen = state
            .choose(&rows, &policy, "model-a", "responses", None)
            .unwrap();
        assert_eq!(chosen.candidate.key(), b);
        drop(chosen);
        state.history.lock().unwrap().get_mut(&a).unwrap().sampled =
            Some(Instant::now() - Duration::from_secs(601));
        assert_eq!(
            state
                .choose(&rows, &policy, "model-a", "responses", None)
                .unwrap()
                .candidate
                .key(),
            a
        );
    }
    #[test]
    fn stream_errors_cancellations_and_partial_frames_do_not_produce_fast_samples() {
        let (models, pools) = fixture();
        let rows = candidates(&models, &pools).unwrap();
        let state = Arc::new(State::default());
        let policy = Policy::default();
        let mut observation = state
            .choose(&rows, &policy, "model-a", "chat_completions", None)
            .unwrap();
        let key = observation.candidate.key();
        observation.response(200, true);
        for part in
            b"data: {\"choices\":[{\"delta\":{\"content\":\"hi\"}}]}\r\n\r\ndata: [DONE]\r\n\r\n"
                .chunks(3)
        {
            observation.feed(part);
        }
        assert!(observation.first.is_some() && observation.terminal);
        drop(observation); // Client canceled: no completed speed sample.
        assert!(state.history.lock().unwrap()[&key].first.is_none());
        assert_eq!(state.history.lock().unwrap()[&key].in_flight, 0);
        let mut observation = state
            .choose(&rows, &policy, "model-a", "chat_completions", None)
            .unwrap();
        observation.response(200, true);
        observation.feed(b"data: {\"type\":\"error\",\"error\":{}}\n\n");
        observation.finish();
        drop(observation);
        assert_ne!(
            state
                .choose(&rows, &policy, "model-a", "chat_completions", None)
                .unwrap()
                .candidate
                .key(),
            key
        );
        let mut observation = state
            .choose(&rows, &policy, "model-a", "responses", None)
            .unwrap();
        observation.response(200, true);
        observation.feed(&vec![b'x'; MAX_BODY + 1]);
        assert!(observation.overflow && observation.buffer.is_empty());
    }
    #[test]
    fn buffered_bursts_and_nonstream_responses_never_claim_a_token_generation_rate() {
        let (models, pools) = fixture();
        let rows = candidates(&models, &pools).unwrap();
        let state = Arc::new(State::default());
        let policy = Policy::default();
        let mut observation = state
            .choose(&rows, &policy, "model-a", "responses", None)
            .unwrap();
        let key = observation.candidate.key();
        observation.response(200, true);
        observation
            .feed(b"data: {\"type\":\"message_start\",\"message\":{\"role\":\"assistant\"}}\n\n");
        assert!(observation.first.is_none());
        observation.feed(b"data: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"a\"}}\n\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"b\"}}\n\ndata: {\"type\":\"message_delta\",\"usage\":{\"output_tokens\":100}}\n\ndata: {\"type\":\"message_stop\"}\n\n");
        assert!(observation.first.is_some() && observation.terminal);
        // A burst shorter than the measurement window has no reliable TPS estimate.
        observation.last = observation.first.map(|first| first + 0.01);
        observation.finish();
        assert!(state.history.lock().unwrap()[&key].first.is_some());
        assert!(state.history.lock().unwrap()[&key].tokens_per_sec.is_none());
        let mut nonstream = state
            .choose(&rows, &policy, "model-a", "chat_completions", None)
            .unwrap();
        nonstream.response(200, false);
        nonstream.feed(b"{\"choices\":[{\"message\":{\"content\":\"hello\"}}],\"usage\":{\"completion_tokens\":100}}");
        nonstream.finish();
        assert!(nonstream.buffer.is_empty());
        let history = state.history.lock().unwrap();
        assert!(history[&nonstream.candidate.key()].first.is_none());
        assert!(history[&nonstream.candidate.key()].tokens_per_sec.is_none());
    }
}
