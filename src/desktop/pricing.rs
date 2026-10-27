//! The management platform serves cached billing prices for available models.
use super::account_client::AccountClient;
use anyhow::{Context, Result, ensure};
use serde_json::Value;

#[derive(Default)]
pub(super) struct Book {
    pub revision: u64,
    pub models: usize,
    pub rows: Vec<Row>,
}
pub(super) struct Row {
    pub provider: &'static str,
    pub model: String,
    pub condition: String,
    pub input: String,
    pub output: String,
    pub cache_read: String,
    pub details: String,
}

pub(super) async fn fetch(client: &AccountClient) -> Result<Book> {
    let value = client
        .request("GET", "pricing", Value::Null, "")
        .await
        .map_err(|e| anyhow::anyhow!("{}", e.message))?;
    ensure!(
        value["scope"] == "available_models",
        "管理平台尚未提供可用模型价格"
    );
    parse(&value)
}

fn text(value: &Value) -> Result<String> {
    let value = value.as_str().context("价格簿文本字段无效")?;
    ensure!(
        !value.is_empty() && value.len() <= 2048 && !value.chars().any(char::is_control),
        "价格簿文本字段无效"
    );
    Ok(value.into())
}
fn dollars(value: &Value, required: bool) -> Result<String> {
    if value.is_null() && !required {
        return Ok("—".into());
    }
    let n = value
        .as_u64()
        .filter(|n| *n <= 1_000_000_000_000)
        .context("价格簿金额无效")?;
    let fraction = format!("{:06}", n % 1_000_000);
    let fraction = fraction.trim_end_matches('0');
    Ok(if fraction.is_empty() {
        (n / 1_000_000).to_string()
    } else {
        format!("{}.{}", n / 1_000_000, fraction)
    })
}
fn checked_date(seconds: u64) -> Result<String> {
    use windows_sys::Win32::{
        Foundation::{FILETIME, SYSTEMTIME},
        System::Time::FileTimeToSystemTime,
    };
    let ticks = seconds
        .checked_mul(10_000_000)
        .and_then(|n| n.checked_add(116_444_736_000_000_000))
        .context("价格簿时间无效")?;
    let file = FILETIME {
        dwLowDateTime: ticks as u32,
        dwHighDateTime: (ticks >> 32) as u32,
    };
    let mut time = SYSTEMTIME::default();
    ensure!(
        unsafe { FileTimeToSystemTime(&file, &mut time) } != 0,
        "价格簿时间无效"
    );
    Ok(format!(
        "{:04}-{:02}-{:02} {:02}:{:02} UTC",
        time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute
    ))
}
fn dimension(value: &Value, default: &str) -> Result<String> {
    if value.is_null() {
        return Ok(default.into());
    }
    Ok(match text(value)?.as_str() {
        "default" | "standard" => "标准".into(),
        "priority" => "优先".into(),
        "flex" => "Flex".into(),
        "scale" => "Scale".into(),
        "fast" => "快速".into(),
        "global" => "全球".into(),
        "us" => "美国".into(),
        other => other.into(),
    })
}
pub(super) fn parse(value: &Value) -> Result<Book> {
    ensure!(
        value["currency"] == "USD" && value["unit"] == "micro_usd_per_million_tokens",
        "不支持的价格币种或单位"
    );
    let mut book = Book {
        revision: value["revision"].as_u64().context("价格簿缺少版本")?,
        ..Default::default()
    };
    let providers = value["providers"].as_array().context("价格簿缺少提供商")?;
    ensure!(providers.len() <= 4, "价格簿提供商过多");
    let mut seen = std::collections::BTreeSet::new();
    for p in providers {
        let provider = match p["provider"].as_str() {
            Some("openai") => "ChatGPT",
            Some("anthropic") => "Claude",
            Some("xai") => "Grok",
            Some("workbuddy") => continue,
            _ => anyhow::bail!("未知价格提供商"),
        };
        let source = text(&p["source"])?;
        let version = p["version"].as_u64().context("价格簿缺少提供商版本")?;
        let checked = checked_date(p["checked_at"].as_u64().context("价格簿缺少核对时间")?)?;
        let models = p["models"].as_array().context("价格簿缺少模型")?;
        ensure!(models.len() <= 8192, "价格簿模型过多");
        for m in models {
            let model = text(&m["model"])?;
            ensure!(seen.insert((provider, model.clone())), "价格簿模型重复");
            book.models += 1;
            let inclusive = m["billing_rules"]["long_context_inclusive"]
                .as_bool()
                .unwrap_or(false);
            let expiry = match m.get("valid_until").filter(|v| !v.is_null()) {
                Some(v) => format!(
                    "\r\n有效期至：{}",
                    checked_date(v.as_u64().context("价格有效期无效")?)?
                ),
                None => String::new(),
            };
            let mut add = |rates: &Value, condition: String| -> Result<()> {
                ensure!(book.rows.len() < 20000, "价格档位过多");
                let row = Row {
                    provider,
                    model: model.clone(),
                    condition,
                    input: dollars(&rates["input"], true)?,
                    output: dollars(&rates["output"], true)?,
                    cache_read: dollars(&rates["cache_read"], false)?,
                    details: format!(
                        "{}：${} / 1 小时缓存写入：${}（每百万 Token；— 表示未提供）\r\n价格版本：{} · 核对时间：{}{}\r\n官方来源：{}",
                        if provider == "Claude" {
                            "5 分钟缓存写入"
                        } else {
                            "缓存写入"
                        },
                        dollars(&rates["cache_write"], false)?,
                        dollars(&rates["cache_write_hour"], false)?,
                        version,
                        checked,
                        expiry,
                        source
                    ),
                };
                book.rows.push(row);
                Ok(())
            };
            let mut group = |rates: &Value, long: &Value, label: String| -> Result<()> {
                if long.is_null() {
                    return add(rates, label);
                }
                let boundary = long["above_input_tokens"]
                    .as_u64()
                    .filter(|n| *n > 0)
                    .context("价格上下文边界无效")?;
                add(
                    rates,
                    format!(
                        "{label} · 输入 {} {boundary}",
                        if inclusive { "<" } else { "≤" }
                    ),
                )?;
                add(
                    &long["rates"],
                    format!(
                        "{label} · 输入 {} {boundary}",
                        if inclusive { "≥" } else { ">" }
                    ),
                )
            };
            group(&m["rates"], &m["long_context"], "标准".into())?;
            if let Some(variants) = m["billing_rules"].get("variants") {
                let variants = variants.as_array().context("价格档位无效")?;
                ensure!(variants.len() <= 32, "价格档位过多");
                for v in variants {
                    group(
                        &v["rates"],
                        &v["long_context"],
                        format!(
                            "{} / {} / {}",
                            dimension(&v["service_tier"], "标准")?,
                            dimension(&v["speed"], "标准")?,
                            dimension(&v["inference_geo"], "全球")?
                        ),
                    )?;
                }
            }
        }
    }
    book.rows
        .sort_by(|a, b| (a.provider, &a.model).cmp(&(b.provider, &b.model)));
    Ok(book)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use serde_json::json;
    pub(crate) fn fixture() -> Value {
        json!({"revision":7,"currency":"USD","unit":"micro_usd_per_million_tokens","providers":[
            {"provider":"openai","version":3,"checked_at":1791936000,"source":"https://developers.openai.com/api/docs/pricing",
             "models":[{"model":"model-without-account","rates":{"input":1250000,"output":10000000,"cache_read":125000}},
                       {"model":"zero-vs-missing","rates":{"input":0,"output":1,"cache_read":null}}]},
            {"provider":"xai","version":1,"checked_at":1791936000,"source":"https://docs.x.ai/developers/pricing",
             "models":[{"model":"grok-test","rates":{"input":2000000,"output":6000000},
                "billing_rules":{"long_context_inclusive":true},"long_context":{"above_input_tokens":200000,"rates":{"input":4000000,"output":12000000}}}]}]})
    }
    #[tokio::test]
    async fn prices_load_from_platform_without_a_router_connection() {
        use axum::{Json, Router, http::HeaderMap, routing::get};
        let mock = Router::new().route(
            "/portal/api/pricing",
            get(|headers: HeaderMap| async move {
                assert!(!headers.contains_key("authorization"));
                let mut book = fixture();
                book["scope"] = json!("available_models");
                // The platform has already filtered models without a usable pool.
                book["providers"][0]["models"] = json!([]);
                Json(book)
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client =
            AccountClient::test_client(format!("http://{}", listener.local_addr().unwrap()));
        let server = tokio::spawn(async {
            axum::serve(listener, mock).await.unwrap();
        });
        let book = fetch(&client).await.unwrap();
        assert_eq!(book.models, 1);
        assert_eq!(book.rows.len(), 2);
        assert!(book.rows.iter().all(|r| r.model == "grok-test"));
        server.abort();
    }
    #[test]
    fn complete_book_preserves_units_missing_prices_and_context_boundaries() {
        let b = parse(&fixture()).unwrap();
        assert_eq!(b.models, 3);
        assert_eq!(b.rows.len(), 4);
        assert_eq!(b.rows[0].input, "1.25");
        assert_eq!(b.rows[0].cache_read, "0.125");
        assert!(b.rows[0].details.contains("2026-10-14"));
        assert_eq!(b.rows[1].input, "0");
        assert_eq!(b.rows[1].output, "0.000001");
        assert_eq!(b.rows[1].cache_read, "—");
        assert!(b.rows[2].condition.contains("< 200000"));
        assert!(b.rows[3].condition.contains("≥ 200000"));
        for (key, value) in [("currency", json!("CNY")), ("unit", json!("usd_per_token"))] {
            let mut bad = fixture();
            bad[key] = value;
            assert!(parse(&bad).is_err());
        }
        let mut bad = fixture();
        bad["providers"][0]["models"][0]["rates"]["input"] = json!(-1);
        assert!(parse(&bad).is_err());
    }
}
