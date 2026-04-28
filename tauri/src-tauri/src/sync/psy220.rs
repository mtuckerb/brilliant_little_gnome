// PSY-220 scraper. Ports `Brilliant::Sync::Psy220ScraperService`.
//
// The Ruby version drove a headless Chromium via Ferrum. To keep this app a
// single Tauri binary, this version does a plain HTML GET with the live
// Brightspace cookie and parses the same grades table with the `scraper`
// crate. Same gating: only runs for course 446900, with a 24h cooldown stored
// in `app_state(key='psy220_last_scrape')`.

use crate::error::Result;
use crate::state::AppState;
use regex::Regex;
use scraper::{Html, Selector};

const PSY220_COURSE_ID: &str = "446900";
const COOLDOWN_SECS: i64 = 86_400;

pub async fn sync(state: &AppState, course_id: &str) -> Result<()> {
    if course_id != PSY220_COURSE_ID { return Ok(()); }

    // Cooldown gate.
    if let Some(last) = read_state(state, "psy220_last_scrape").await? {
        if let Ok(t) = chrono::DateTime::parse_from_rfc3339(&last) {
            let elapsed = chrono::Utc::now().timestamp() - t.timestamp();
            if elapsed < COOLDOWN_SECS {
                tracing::info!("[psy220] cooldown — last scrape {}s ago", elapsed);
                return Ok(());
            }
        }
    }

    let path = format!("/d2l/lms/grades/my_grades/main.d2l?ou={}", course_id);
    let html = match state.client.fetch_html(&path).await {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("[psy220] fetch failed: {}", e);
            return Ok(());
        }
    };

    // Parse synchronously into Send-safe owned rows; `scraper::Html` isn't Send,
    // so it must be dropped before any await.
    let rows = parse_rows(&html);
    let Some(rows) = rows else {
        tracing::warn!("[psy220] grades table not found — cookies may be expired");
        return Ok(());
    };

    let mut scraped = 0usize;
    for r in rows {
        let ScrapedRow { name, displayed, numerator, denominator, weight, is_extra, bs_id } = r;
        sqlx::query(
            "INSERT INTO grades (course_id, brightspace_id, name, displayed_grade, numerator, denominator, weight, is_extra_credit, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
             ON CONFLICT(course_id, brightspace_id) DO UPDATE SET
                name = excluded.name,
                displayed_grade = COALESCE(excluded.displayed_grade, grades.displayed_grade),
                numerator = COALESCE(excluded.numerator, grades.numerator),
                denominator = COALESCE(excluded.denominator, grades.denominator),
                weight = COALESCE(excluded.weight, grades.weight),
                is_extra_credit = excluded.is_extra_credit,
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind(course_id)
        .bind(&bs_id)
        .bind(&name)
        .bind(displayed)
        .bind(numerator)
        .bind(denominator)
        .bind(weight)
        .bind(is_extra as i64)
        .execute(&state.pool)
        .await?;
        scraped += 1;
    }

    write_state(state, "psy220_last_scrape", &chrono::Utc::now().to_rfc3339()).await?;
    tracing::info!("[psy220] scraped {} rows", scraped);
    Ok(())
}

async fn read_state(state: &AppState, key: &str) -> Result<Option<String>> {
    let row: Option<(Option<String>,)> = sqlx::query_as("SELECT value FROM app_state WHERE key = ?")
        .bind(key)
        .fetch_optional(&state.pool)
        .await?;
    Ok(row.and_then(|r| r.0))
}

async fn write_state(state: &AppState, key: &str, value: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO app_state (key, value, updated_at) VALUES (?, ?, CURRENT_TIMESTAMP)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP",
    )
    .bind(key)
    .bind(value)
    .execute(&state.pool)
    .await?;
    Ok(())
}

struct ScrapedRow {
    name: String,
    displayed: Option<String>,
    numerator: Option<f64>,
    denominator: Option<f64>,
    weight: Option<f64>,
    is_extra: bool,
    bs_id: String,
}

fn parse_rows(html: &str) -> Option<Vec<ScrapedRow>> {
    let doc = Html::parse_document(html);
    let table_sel = Selector::parse(r#"table[summary="List of grade items and their values"]"#).unwrap();
    let row_sel = Selector::parse("tr").unwrap();
    let cell_sel = Selector::parse("th, td").unwrap();
    let drh_re = Regex::new(r"drh\(\s*\d+\s*,\s*(\d+)\s*\)").unwrap();
    let frac_re = Regex::new(r"([\d\.]+)\s*/\s*([\d\.]+)").unwrap();
    let denom_re = Regex::new(r"/\s*([\d\.]+)").unwrap();
    let num_re = Regex::new(r"[\d\.]+").unwrap();

    let table = doc.select(&table_sel).next()?;
    let mut out: Vec<ScrapedRow> = Vec::new();

    for row in table.select(&row_sel) {
        let row_html = row.html();
        let id = drh_re.captures(&row_html).and_then(|c| c.get(1)).map(|m| m.as_str().to_string());
        let cells: Vec<String> = row.select(&cell_sel)
            .map(|c| c.text().collect::<String>().trim().to_string())
            .collect();

        let (name, points_str, weight_str, grade_str) = match cells.len() {
            n if n >= 5 => {
                let name = if cells[0].is_empty() { cells[1].clone() } else { cells[0].clone() };
                (name, Some(cells[2].clone()), Some(cells[3].clone()), Some(cells[4].clone()))
            }
            4 => (cells[0].clone(), Some(cells[2].clone()), None, Some(cells[3].clone())),
            _ => continue,
        };

        if name.is_empty() || name == "Grade Item" { continue; }

        let (numerator, denominator) = match points_str.as_deref() {
            Some(s) => {
                if let Some(c) = frac_re.captures(s) {
                    let n: Option<f64> = c.get(1).and_then(|m| m.as_str().parse().ok());
                    let d: Option<f64> = c.get(2).and_then(|m| m.as_str().parse().ok());
                    (n, d)
                } else if let Some(c) = denom_re.captures(s) {
                    let d: Option<f64> = c.get(1).and_then(|m| m.as_str().parse().ok());
                    (None, d)
                } else { (None, None) }
            }
            None => (None, None),
        };

        let weight: Option<f64> = weight_str.as_deref()
            .and_then(|s| num_re.find(s).and_then(|m| m.as_str().parse().ok()));
        let displayed = grade_str.as_ref().filter(|s| !s.is_empty() && s.as_str() != "-%").cloned();
        let bs_id = id.unwrap_or_else(|| format!("scraped_{}", md5_hex8(&name)));
        let is_extra = name.contains("(Bonus)");

        out.push(ScrapedRow { name, displayed, numerator, denominator, weight, is_extra, bs_id });
    }
    Some(out)
}

fn md5_hex8(s: &str) -> String {
    // We don't have md5 in deps; substitute a deterministic 8-hex-char hash.
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut h = DefaultHasher::new();
    s.hash(&mut h);
    format!("{:08x}", (h.finish() & 0xFFFFFFFF) as u32)
}
