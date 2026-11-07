use std::sync::Arc;
use std::time::Duration;

use noctics_shared::{PINTEREST_API_PORT, Pin, PinterestError, pin_page};
use serde_json::{Value, json};
use url::Url;

use crate::local_api::{self, Reply, Search};
use crate::pinterest::Pinterest;

const BASE_TIMEOUT: Duration = Duration::from_secs(25);
const TIMEOUT_PER_PIN: Duration = Duration::from_millis(500);
const MAX_TIMEOUT: Duration = Duration::from_secs(90);
const MISSING_QUERY: &str = "Додай запит: /pins?q=cyberpunk+ui";
const SIGNED_OUT_MESSAGE: &str = "Pinterest нічого не віддав. Увійди в Pinterest у Noctics і спробуй ще раз.";

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Help,
    Health,
    Pins(Search),
    BadQuery(&'static str),
    NotFound,
}

pub fn serve(pinterest: Arc<Pinterest>) {
    local_api::serve(PINTEREST_API_PORT, move |address| answer(&pinterest, address));
}

fn answer(pinterest: &Pinterest, address: &Url) -> Reply {
    match route(address) {
        Route::Help => Reply::new(200, help()),
        Route::Health => Reply::new(200, json!({ "ok": true, "signed_in": pinterest.status().signed_in })),
        Route::Pins(Search { query, limit }) => search(pinterest, &query, limit),
        Route::BadQuery(message) => Reply::error(400, "bad_query", message),
        Route::NotFound => Reply::not_found(),
    }
}

fn route(address: &Url) -> Route {
    match address.path() {
        "/" => Route::Help,
        "/health" => Route::Health,
        "/pins" => local_api::search_terms(address, MISSING_QUERY).map_or_else(Route::BadQuery, Route::Pins),
        _ => Route::NotFound,
    }
}

fn search(pinterest: &Pinterest, query: &str, limit: usize) -> Reply {
    let signed_in = pinterest.status().signed_in;
    match pinterest.search_pins(query, limit, timeout_for(limit)) {
        Ok(pins) if pins.is_empty() && signed_in != Some(true) => {
            Reply::error(401, "signed_out", SIGNED_OUT_MESSAGE)
        }
        Ok(pins) => Reply::new(
            200,
            json!({
                "query": query,
                "count": pins.len(),
                "signed_in": signed_in,
                "pins": pins.iter().map(pin_json).collect::<Vec<_>>(),
            }),
        ),
        Err(PinterestError::Browser(detail)) => Reply::error(502, "browser", &detail),
        Err(other) => Reply::error(500, "internal", &other.to_string()),
    }
}

fn pin_json(pin: &Pin) -> Value {
    json!({
        "id": pin.id,
        "image": pin.image,
        "title": pin.title,
        "link": pin_page(&pin.id),
    })
}

fn help() -> Value {
    json!({
        "name": "Noctics Pinterest API",
        "free": true,
        "address": local_api::address(PINTEREST_API_PORT),
        "endpoints": {
            "/pins?q=<запит>&limit=<1-100>": "Піни з пошуку Pinterest: id, image, title, link",
            "/health": "Чи працює API і чи ввійдено в Pinterest",
        },
        "note": "Працює, поки відкрито Noctics. Запити виконуються по одному.",
    })
}

fn timeout_for(limit: usize) -> Duration {
    let extra = TIMEOUT_PER_PIN.saturating_mul(u32::try_from(limit).unwrap_or(u32::MAX));
    (BASE_TIMEOUT + extra).min(MAX_TIMEOUT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_api::{DEFAULT_LIMIT, MAX_LIMIT, local};

    #[test]
    fn routes_pin_searches() {
        assert_eq!(
            route(&local("/pins?q=cyberpunk+%20ui&limit=40")),
            Route::Pins(Search {
                query: "cyberpunk ui".into(),
                limit: 40
            })
        );
        assert_eq!(
            route(&local("/pins?q=neon")),
            Route::Pins(Search {
                query: "neon".into(),
                limit: DEFAULT_LIMIT
            })
        );
        assert_eq!(route(&local("/pins")), Route::BadQuery(MISSING_QUERY));
        assert!(matches!(route(&local("/pins?q=a&limit=101")), Route::BadQuery(_)));
        assert_eq!(route(&local("/")), Route::Help);
        assert_eq!(route(&local("/health")), Route::Health);
        assert_eq!(route(&local("/admin")), Route::NotFound);
    }

    #[test]
    fn scales_timeouts_with_the_limit() {
        assert_eq!(timeout_for(10), BASE_TIMEOUT + Duration::from_secs(5));
        assert_eq!(timeout_for(MAX_LIMIT), Duration::from_secs(75));
        assert_eq!(timeout_for(10_000), MAX_TIMEOUT);
    }
}
