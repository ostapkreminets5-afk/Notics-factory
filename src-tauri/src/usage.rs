use std::path::PathBuf;
use std::time::Duration;

use chrono::{DateTime, Datelike, FixedOffset, Local, TimeZone, Timelike};
use noctics_shared::{ClaudeUsage, UsageWindow};
use serde_json::Value;

const USAGE_URL: &str = "https://api.anthropic.com/api/oauth/usage";
const OAUTH_BETA: &str = "oauth-2025-04-20";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONFIG_DIR_VARIABLE: &str = "CLAUDE_CONFIG_DIR";
const DEFAULT_CONFIG_DIR: &str = ".claude";
const CREDENTIALS_FILE: &str = ".credentials.json";
const SESSION_WINDOW: &str = "five_hour";
const WEEKLY_WINDOW: &str = "seven_day";
const MAX_PERCENT: f64 = 100.0;
const WEEKDAYS: [&str; 7] = ["пн", "вт", "ср", "чт", "пт", "сб", "нд"];
const MONTHS: [&str; 12] = [
    "січ.", "лют.", "бер.", "квіт.", "трав.", "черв.", "лип.", "серп.", "вер.", "жовт.", "лист.",
    "груд.",
];

pub fn fetch() -> Option<ClaudeUsage> {
    let token = access_token()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .into();
    let mut response = agent
        .get(USAGE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", OAUTH_BETA)
        .call()
        .ok()?;
    let body = response.body_mut().read_to_string().ok()?;
    let report = serde_json::from_str::<Value>(&body).ok()?;
    Some(parse(&report, &Local::now()))
}

fn access_token() -> Option<String> {
    let text = std::fs::read_to_string(config_dir()?.join(CREDENTIALS_FILE)).ok()?;
    let credentials = serde_json::from_str::<Value>(&text).ok()?;
    credentials["claudeAiOauth"]["accessToken"]
        .as_str()
        .filter(|token| !token.is_empty())
        .map(str::to_owned)
}

fn config_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os(CONFIG_DIR_VARIABLE) {
        return Some(PathBuf::from(dir));
    }
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|home| PathBuf::from(home).join(DEFAULT_CONFIG_DIR))
}

fn parse<Tz: TimeZone>(report: &Value, now: &DateTime<Tz>) -> ClaudeUsage {
    ClaudeUsage {
        session: window(&report[SESSION_WINDOW], now),
        weekly: window(&report[WEEKLY_WINDOW], now),
    }
}

fn window<Tz: TimeZone>(entry: &Value, now: &DateTime<Tz>) -> Option<UsageWindow> {
    let utilization = entry["utilization"].as_f64()?;
    let resets = entry["resets_at"]
        .as_str()
        .and_then(|moment| DateTime::parse_from_rfc3339(moment).ok())
        .map(|moment| reset_label(&moment, now));
    Some(UsageWindow {
        percent: utilization.clamp(0.0, MAX_PERCENT).round() as u32,
        resets,
    })
}

fn reset_label<Tz: TimeZone>(moment: &DateTime<FixedOffset>, now: &DateTime<Tz>) -> String {
    let local = moment.with_timezone(&now.timezone());
    let time = format!("{:02}:{:02}", local.hour(), local.minute());
    if local.date_naive() == now.date_naive() {
        return format!("сьогодні о {time}");
    }
    let weekday = WEEKDAYS[local.weekday().num_days_from_monday() as usize];
    let month = MONTHS[local.month0() as usize];
    format!("{weekday}, {} {month} о {time}", local.day())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_session_and_weekly_windows() {
        let now = Utc.with_ymd_and_hms(2026, 11, 2, 1, 0, 0).unwrap();
        let report = json!({
            "five_hour": { "utilization": 80.6, "resets_at": "2026-11-02T04:10:00+00:00" },
            "seven_day": { "utilization": 11.0, "resets_at": "2026-11-08T10:00:00.5+00:00" },
            "seven_day_opus": null
        });
        let usage = parse(&report, &now);
        assert_eq!(
            usage.session,
            Some(UsageWindow {
                percent: 81,
                resets: Some("сьогодні о 04:10".into()),
            })
        );
        assert_eq!(
            usage.weekly,
            Some(UsageWindow {
                percent: 11,
                resets: Some("нд, 8 лист. о 10:00".into()),
            })
        );
    }

    #[test]
    fn tolerates_missing_or_odd_windows() {
        let now = Utc.with_ymd_and_hms(2026, 11, 2, 1, 0, 0).unwrap();
        let report = json!({
            "five_hour": { "utilization": 140.0, "resets_at": null },
            "seven_day": null
        });
        let usage = parse(&report, &now);
        assert_eq!(
            usage.session,
            Some(UsageWindow {
                percent: 100,
                resets: None,
            })
        );
        assert_eq!(usage.weekly, None);
    }
}
