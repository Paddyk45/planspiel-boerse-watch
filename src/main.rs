#![warn(clippy::nursery, clippy::pedantic)]
#![allow(
    clippy::too_many_lines,
    clippy::missing_panics_doc,
    clippy::must_use_candidate
)]

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::env;
use std::fmt::Write as _;
use std::sync::OnceLock;
use std::time::{Duration, UNIX_EPOCH};

static PSB_TOKEN: OnceLock<String> = OnceLock::new();
static DISCORD_WEBHOOK: OnceLock<String> = OnceLock::new();

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv()?;

    PSB_TOKEN.set(env::var("PSB_TOKEN")?).unwrap();
    DISCORD_WEBHOOK.set(env::var("DISCORD_WEBHOOK")?).unwrap();

    watcher().await;
    Ok(())
}

async fn webhook(content: String, message_id: Option<u64>) -> anyhow::Result<u64> {
    if let Some(message_id) = message_id {
        reqwest::Client::new()
            .patch(format!(
                "{}/messages/{message_id}",
                DISCORD_WEBHOOK.get().unwrap()
            ))
            .json(&json!({"content": content}))
            .send()
            .await?;

        return Ok(message_id);
    }
    let res: serde_json::Value = reqwest::Client::new()
        .post(format!("{}?wait=true", DISCORD_WEBHOOK.get().unwrap()))
        .json(&json!({"content": content}))
        .send()
        .await?
        .json()
        .await?;

    Ok(res["id"].as_str().unwrap().parse()?)
}

const MARKET_URL: &str = "https://trading.planspiel-boerse.de/stockcontest/services/api/v-ms1/instrument/getAllInstruments";
const RANKING_URL: &str =
    "https://backstage.planspiel-boerse.de/stockcontest/services/api/v-ms6/ranking/getRanking";
const INTERVAL: u64 = 60 * 15; // 15 min

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
struct Instrument {
    name: String,
    id_external: String,
    wkn: String,
    price: f32,
    performance_abs: f32,
    performance_rel: f32,
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[derive(Debug)]
struct Team {
    name: String,
    depot_value: f32,
    performance: f32,
    performance_rank: u32,
}
pub fn current_unix_time() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

async fn check_instruments(
    client: &reqwest::Client,
    instrument_cache: &mut Vec<HashMap<u64, Instrument>>,
    rel_message_id: u64,
    perf_diff_message_id: u64,
) {
    let res: serde_json::Value = match client
        .get(MARKET_URL)
        .bearer_auth(PSB_TOKEN.get().unwrap())
        .send()
        .await
    {
        Ok(res) => {
            let Ok(j) = res.json().await else {
                eprintln!("failed to parse json");
                return;
            };

            j
        }
        Err(e) => {
            eprintln!("response error: {e}");
            return;
        }
    };

    let mut instruments: Vec<Instrument> = vec![];
    for idx in res.as_array().unwrap() {
        for ins in idx.get("instrumentList").unwrap().as_array().unwrap() {
            let Ok(val) = serde_json::from_value(ins.clone()) else {
                continue;
            };
            instruments.push(val);
        }
    }

    instruments.sort_by(|i, j| j.performance_rel.total_cmp(&i.performance_rel));
    instruments.dedup_by(|i, j| i.wkn == j.wkn);

    // result store
    let mut hm = HashMap::new();
    for ins in &instruments {
        hm.insert(ins.id_external.parse().unwrap(), ins.clone());
    }

    instrument_cache.push(hm);

    let len = instrument_cache.len();
    let cache_1h = if len >= 4 {
        Some(instrument_cache.get(len - 4 - 1).unwrap().clone())
    } else {
        None
    };

    let cache_2h = if len >= 8 {
        Some(instrument_cache.get(len - 8 - 1).unwrap().clone())
    } else {
        None
    };

    let cache_4h = if len > 16 {
        Some(instrument_cache.remove(0))
    } else {
        None
    };

    let mut cont = String::new();
    cont.push_str("top 15 kurse (nach rel. veränderung)\n");
    for inst in &instruments[..15] {
        let _ = writeln!(
            cont,
            "`* {} ({}% [{}€]) ({}% 1h, {}% 2h, {}% 4h)`",
            inst.name,
            inst.performance_rel,
            inst.performance_abs,
            cache_1h.as_ref().map_or_else(
                || "?".to_string(),
                |c| c
                    .get(&inst.id_external.parse().unwrap())
                    .unwrap()
                    .performance_rel
                    .to_string()
            ),
            cache_2h.as_ref().map_or_else(
                || "?".to_string(),
                |c| c
                    .get(&inst.id_external.parse().unwrap())
                    .unwrap()
                    .performance_rel
                    .to_string()
            ),
            cache_4h.as_ref().map_or_else(
                || "?".to_string(),
                |c| c
                    .get(&inst.id_external.parse().unwrap())
                    .unwrap()
                    .performance_rel
                    .to_string()
            ),
        );
    }

    cont.push('\n');

    instruments.reverse();
    cont.push_str("bottom 10 kurse (nach rel. veränderung)\n");
    for inst in &instruments[..10] {
        let _ = writeln!(
            cont,
            "`* {} ({}% [{}€]) ({}% vor 1h, {}% vor 2h, {}% vor 4h)`",
            inst.name,
            inst.performance_rel,
            inst.performance_abs,
            cache_1h.as_ref().map_or_else(
                || "?".to_string(),
                |c| c
                    .get(&inst.id_external.parse().unwrap())
                    .unwrap()
                    .performance_rel
                    .to_string()
            ),
            cache_2h.as_ref().map_or_else(
                || "?".to_string(),
                |c| c
                    .get(&inst.id_external.parse().unwrap())
                    .unwrap()
                    .performance_rel
                    .to_string()
            ),
            cache_4h.as_ref().map_or_else(
                || "?".to_string(),
                |c| c
                    .get(&inst.id_external.parse().unwrap())
                    .unwrap()
                    .performance_rel
                    .to_string()
            ),
        );
    }

    let _ = write!(cont, "\n\naktualisiert: <t:{}>", current_unix_time());

    if let Err(e) = webhook(cont.clone(), Some(rel_message_id)).await {
        eprintln!("webhook error: {e:?}");
    }

    // *****************************

    cont.clear();

    if let Some(cache) = cache_2h {
        let mut instruments_rel_rel_perf = vec![];
        for ins in instruments {
            let Some(old) = cache.get(&ins.id_external.parse().unwrap()) else {
                continue;
            };

            let diff = ins.performance_rel - old.performance_rel;

            instruments_rel_rel_perf.push((ins.id_external.parse::<u64>().unwrap(), diff));
        }

        instruments_rel_rel_perf.sort_by(|(_, i), (_, j)| j.total_cmp(i));

        cont.clear();
        cont.push_str("top 20 kurse (nach diff. in rel. veränderung vor 2h)\n");

        for (id, diff) in &instruments_rel_rel_perf[..20] {
            let ins = cache.get(id).unwrap();
            let _ = writeln!(cont, "`* {} (Δ: {}%)`", ins.name, diff);
        }

        let _ = write!(cont, "\n\naktualisiert: <t:{}>", current_unix_time());

        if let Err(e) = webhook(cont.clone(), Some(perf_diff_message_id)).await {
            eprintln!("webhook error: {e:?}");
        }
    }
}

async fn check_leaderboard(client: &reqwest::Client, message_id: u64) {
    let res: serde_json::Value = match client
        .post(RANKING_URL)
        .json(&json!({"additionalRanking": false, "filter": "COUNTRY", "name": "", "page": 0, "pageSize": i32::MAX, "periodDetail": 278, "periodType": "TOTAL", "periodYear": 2025, "phaseId": "1", "rankingColumn": "PERFORMANCE"}))
        .bearer_auth(PSB_TOKEN.get().unwrap())
        .send()
        .await {
        Ok(res) => {
            let Ok(j) = res.json().await else {
                eprintln!("failed to parse json");
                return;
            };

            j
        },
        Err(e) => {
            eprintln!("{e}");
            return;
        }
    };

    let ranking_total_elements: u64 = res["totalElements"].as_number().unwrap().as_u64().unwrap();

    let mut ranking: Vec<Team> = serde_json::from_value(res["content"].clone()).unwrap();

    ranking.sort_by(|r, s| s.depot_value.total_cmp(&r.depot_value));

    let mut cont = String::new();
    cont.push_str("top 10 teams (nach depot)\n");
    for team in &ranking[..10] {
        let _ = writeln!(
            cont,
            "`* {} ({}€, #{} / {}%)`",
            team.name.replace("* (Name not yet approved)", "<kein>"),
            team.depot_value,
            team.performance_rank,
            team.performance,
        );
    }

    cont.push('\n');

    ranking.reverse();
    cont.push_str("bottom 10 teams (nach depot)\n");
    for team in &ranking[..10] {
        let _ = writeln!(
            cont,
            "`* {} ({}€, #{} / {}%)`",
            team.name.replace("* (Name not yet approved)", "<kein>"),
            team.depot_value,
            team.performance_rank,
            team.performance,
        );
    }

    let _ = write!(cont, "\n({ranking_total_elements} teams insgesamt)");
    let _ = write!(cont, "\n\naktualisiert: <t:{}>", current_unix_time());

    if let Err(e) = webhook(cont.clone(), Some(message_id)).await {
        eprintln!("webhook error: {e:?}");
    }
}

async fn watcher() {
    let mut instrument_cache: Vec<HashMap<u64, Instrument, _>> = vec![];
    let client = reqwest::Client::new();

    let instrument_message_id = if let Ok(msg_id) = env::var("DISCORD_INSTRUMENT_MSG_ID") {
        msg_id.parse().unwrap()
    } else {
        webhook("tmp".to_string(), None)
            .await
            .expect("failed to init webhook")
    };

    let instrument_perf_diff_message_id =
        if let Ok(msg_id) = env::var("DISCORD_INSTRUMENT_PERF_DIFF_MSG_ID") {
            msg_id.parse().unwrap()
        } else {
            webhook("tmp".to_string(), None)
                .await
                .expect("failed to init webhook")
        };

    let teams_message_id = if let Ok(msg_id) = env::var("DISCORD_TEAMS_MSG_ID") {
        msg_id.parse().unwrap()
    } else {
        webhook("tmp".to_string(), None)
            .await
            .expect("failed to init webhook")
    };

    check_instruments(
        &client,
        &mut instrument_cache,
        instrument_message_id,
        instrument_perf_diff_message_id,
    )
    .await;
    check_leaderboard(&client, teams_message_id).await;

    loop {
        tokio::time::sleep(Duration::from_secs(INTERVAL)).await;

        check_instruments(
            &client,
            &mut instrument_cache,
            instrument_message_id,
            instrument_perf_diff_message_id,
        )
        .await;
        check_leaderboard(&client, teams_message_id).await;
    }
}
