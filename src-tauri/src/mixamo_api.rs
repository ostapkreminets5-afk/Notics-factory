use std::sync::Arc;

use noctics_shared::{
    DEFAULT_MOTION_FPS, FULL_TRIM, MIXAMO_API_PORT, MixamoError, MotionClip, MotionDraft, MotionOptions, MotionOrder,
};
use serde_json::{Value, json};
use url::Url;

use crate::local_api::{self, Reply, Search};
use crate::mixamo::Mixamo;

const MISSING_QUERY: &str = "Додай запит: /search?q=sword+slash";
const MISSING_ORDER: &str = "Додай q (що шукати на Mixamo) або id анімації з /search: /order?name=run&q=running&in_place=1";
const BAD_PROJECT: &str = "project_id має бути числом.";
const BAD_ID: &str = "Номер замовлення має бути числом.";
const TRUE_WORDS: [&str; 4] = ["1", "true", "yes", "on"];

#[derive(Debug, PartialEq)]
enum Route {
    Help,
    Health,
    Search(Search),
    Order(MotionDraft),
    Plan(i64),
    List(Option<i64>),
    One(i64),
    Retry(i64),
    BadQuery(&'static str),
    NotFound,
}

pub fn serve(mixamo: Arc<Mixamo>) {
    local_api::serve(MIXAMO_API_PORT, move |address| answer(&mixamo, address));
}

fn answer(mixamo: &Mixamo, address: &Url) -> Reply {
    match route(address) {
        Route::Help => Reply::new(200, help()),
        Route::Health => {
            let status = mixamo.status();
            Reply::new(
                200,
                json!({ "ok": true, "signed_in": status.signed_in, "character": status.character }),
            )
        }
        Route::Search(Search { query, limit }) => match mixamo.search(&query, limit) {
            Ok(clips) => Reply::new(
                200,
                json!({ "query": query, "count": clips.len(), "animations": clips.iter().map(clip_json).collect::<Vec<_>>() }),
            ),
            Err(problem) => failure(&problem),
        },
        Route::Order(draft) => match mixamo.create(&draft) {
            Ok(order) => Reply::new(201, json!({ "order": order_json(&order) })),
            Err(problem) => failure(&problem),
        },
        Route::Plan(project_id) => match mixamo.plan_project(project_id) {
            Ok(orders) => Reply::new(201, json!({ "orders": orders.iter().map(order_json).collect::<Vec<_>>() })),
            Err(problem) => failure(&problem),
        },
        Route::List(project_id) => match mixamo.orders(project_id) {
            Ok(orders) => Reply::new(200, json!({ "orders": orders.iter().map(order_json).collect::<Vec<_>>() })),
            Err(problem) => failure(&problem),
        },
        Route::One(id) => match mixamo.order(id) {
            Ok(order) => Reply::new(200, json!({ "order": order_json(&order) })),
            Err(problem) => failure(&problem),
        },
        Route::Retry(id) => match mixamo.retry(id).and_then(|()| mixamo.order(id)) {
            Ok(order) => Reply::new(200, json!({ "order": order_json(&order) })),
            Err(problem) => failure(&problem),
        },
        Route::BadQuery(message) => Reply::error(400, "bad_query", message),
        Route::NotFound => Reply::not_found(),
    }
}

fn route(address: &Url) -> Route {
    let segments: Vec<&str> = address
        .path()
        .trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect();
    match segments.as_slice() {
        [] => Route::Help,
        ["health"] => Route::Health,
        ["search"] => local_api::search_terms(address, MISSING_QUERY).map_or_else(Route::BadQuery, Route::Search),
        ["order"] => order_route(address),
        ["plan"] => match number(address, "project_id") {
            Ok(Some(project_id)) => Route::Plan(project_id),
            _ => Route::BadQuery(BAD_PROJECT),
        },
        ["animations"] => match number(address, "project_id") {
            Ok(project_id) => Route::List(project_id),
            Err(message) => Route::BadQuery(message),
        },
        ["animations", id] => id.parse().map_or(Route::BadQuery(BAD_ID), Route::One),
        ["animations", id, "retry"] => id.parse().map_or(Route::BadQuery(BAD_ID), Route::Retry),
        _ => Route::NotFound,
    }
}

fn order_route(address: &Url) -> Route {
    let text = |name: &str| parameter(address, name).unwrap_or_default();
    let flag = |name: &str| {
        parameter(address, name).is_some_and(|value| TRUE_WORDS.contains(&value.trim().to_ascii_lowercase().as_str()))
    };
    let project_id = match number(address, "project_id") {
        Ok(project_id) => project_id,
        Err(message) => return Route::BadQuery(message),
    };
    let fps = parameter(address, "fps")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_MOTION_FPS);
    let (trim_start, trim_end) = parameter(address, "trim")
        .and_then(|value| {
            let (start, end) = value.split_once('-')?;
            Some((start.trim().parse().ok()?, end.trim().parse().ok()?))
        })
        .unwrap_or((0, FULL_TRIM));
    let clip_id = parameter(address, "id").filter(|id| !id.trim().is_empty());
    let query = text("q");
    if query.trim().is_empty() && clip_id.is_none() {
        return Route::BadQuery(MISSING_ORDER);
    }
    Route::Order(MotionDraft {
        name: text("name"),
        query,
        clip_id,
        project_id,
        options: MotionOptions {
            in_place: flag("in_place"),
            mirror: flag("mirror"),
            skin: flag("skin"),
            fps,
            trim_start,
            trim_end,
        },
    })
}

fn parameter(address: &Url, name: &str) -> Option<String> {
    address
        .query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

fn number(address: &Url, name: &str) -> Result<Option<i64>, &'static str> {
    match parameter(address, name) {
        None => Ok(None),
        Some(raw) if raw.trim().is_empty() => Ok(None),
        Some(raw) => raw.trim().parse().map(Some).map_err(|_| BAD_PROJECT),
    }
}

fn failure(problem: &MixamoError) -> Reply {
    match problem {
        MixamoError::SignedOut => Reply::error(
            401,
            "signed_out",
            "Mixamo не підключено. Увійди в Mixamo в Noctics (розділ Mixamo або вузол «Анімації»).",
        ),
        MixamoError::NotFound => Reply::error(404, "not_found", "Такої анімації, замовлення чи гри немає."),
        MixamoError::NoMatch => Reply::error(404, "no_match", "Mixamo нічого не знайшов — спробуй інший запит англійською."),
        MixamoError::QueryMissing => Reply::error(400, "bad_query", MISSING_ORDER),
        MixamoError::QueryTooLong | MixamoError::NameTooLong | MixamoError::NameMissing => {
            Reply::error(400, "bad_query", &problem.to_string())
        }
        MixamoError::Network(detail) => Reply::error(502, "mixamo", detail),
        other => Reply::error(500, "internal", &other.to_string()),
    }
}

fn clip_json(clip: &MotionClip) -> Value {
    json!({
        "id": clip.id,
        "name": clip.name,
        "description": clip.description,
        "preview": clip.preview(),
        "supports_in_place": clip.supports_in_place,
    })
}

fn order_json(order: &MotionOrder) -> Value {
    json!({
        "id": order.id,
        "name": order.name,
        "query": order.query,
        "project_id": order.project_id,
        "status": order.status.code(),
        "label": order.status.label(),
        "animation": order.clip.as_ref().map(clip_json),
        "options": order.options,
        "file": order.file,
        "size_bytes": order.size_bytes,
        "error": order.error,
        "follow": format!("/animations/{}", order.id),
    })
}

fn help() -> Value {
    json!({
        "name": "Noctics Mixamo API",
        "address": local_api::address(MIXAMO_API_PORT),
        "idea": "Анімації з mixamo.com на персонажі Mixamo: пошук, вибір найближчої, запікання з налаштуваннями й FBX на диску.",
        "endpoints": {
            "/search?q=<англійською>&limit=<1-100>": "Анімації Mixamo, відсортовані за схожістю з запитом: id, name, description, preview",
            "/order?name=<назва>&q=<запит>&id=<id з /search>&project_id=<гра>&in_place=1&mirror=1&skin=1&fps=24|30|60&trim=0-100": "Замовити анімацію. Без id Noctics сам обере найближчу до q. Повтор того самого замовлення повертає наявне",
            "/plan?project_id=<гра>": "Підібрати набір анімацій за дизайн-документом гри й поставити в чергу",
            "/animations?project_id=<гра>": "Замовлення та їхній стан",
            "/animations/<id>": "Одне замовлення: status (queued, searching, exporting, downloading, done, failed), file — шлях до FBX",
            "/animations/<id>/retry": "Поставити замовлення зі статусом failed або done знову в чергу",
            "/health": "Чи працює API і чи підключено Mixamo",
        },
        "note": "Працює, поки відкрито Noctics. Замовлення виконуються по одному. FBX — 30 fps, без скіна, якщо не попросиш skin=1.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_api::local;

    #[test]
    fn routes_mixamo_requests() {
        assert_eq!(route(&local("/")), Route::Help);
        assert_eq!(route(&local("/health")), Route::Health);
        assert_eq!(
            route(&local("/search?q=sword+slash&limit=5")),
            Route::Search(Search {
                query: "sword slash".into(),
                limit: 5
            })
        );
        assert_eq!(route(&local("/search")), Route::BadQuery(MISSING_QUERY));
        assert_eq!(route(&local("/plan?project_id=4")), Route::Plan(4));
        assert_eq!(route(&local("/plan")), Route::BadQuery(BAD_PROJECT));
        assert_eq!(route(&local("/animations?project_id=4")), Route::List(Some(4)));
        assert_eq!(route(&local("/animations")), Route::List(None));
        assert_eq!(route(&local("/animations/12")), Route::One(12));
        assert_eq!(route(&local("/animations/x")), Route::BadQuery(BAD_ID));
        assert_eq!(route(&local("/animations/12/retry")), Route::Retry(12));
        assert_eq!(route(&local("/animations/x/retry")), Route::BadQuery(BAD_ID));
        assert_eq!(route(&local("/pins")), Route::NotFound);
    }

    #[test]
    fn reads_orders_from_the_query_string() {
        let Route::Order(draft) = route(&local(
            "/order?name=run&q=fast+run&project_id=7&in_place=1&mirror=true&fps=60&trim=10-90",
        )) else {
            panic!("expected an order");
        };
        assert_eq!(draft.name, "run");
        assert_eq!(draft.query, "fast run");
        assert_eq!(draft.project_id, Some(7));
        assert!(draft.options.in_place && draft.options.mirror && !draft.options.skin);
        assert_eq!(draft.options.fps, 60);
        assert_eq!((draft.options.trim_start, draft.options.trim_end), (10, 90));
        let Route::Order(picked) = route(&local("/order?name=attack&id=abc-1")) else {
            panic!("expected an order");
        };
        assert_eq!(picked.clip_id.as_deref(), Some("abc-1"));
        assert_eq!(picked.options, MotionOptions::default());
        assert_eq!(route(&local("/order?name=run")), Route::BadQuery(MISSING_ORDER));
        assert_eq!(route(&local("/order?q=run&project_id=x")), Route::BadQuery(BAD_PROJECT));
    }
}
