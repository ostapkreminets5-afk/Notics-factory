use std::sync::Arc;

use noctics_shared::{ARTSTATION_API_PORT, ArtstationError, Artwork, DETAIL_ARTWORK_SIZE};
use serde_json::{Value, json};
use url::Url;

use crate::artstation::ArtStation;
use crate::local_api::{self, Reply, Search};

const MISSING_QUERY: &str = "Додай запит: /artworks?q=dark+fantasy+castle";

#[derive(Debug, PartialEq, Eq)]
enum Route {
    Help,
    Health,
    Artworks(Search),
    BadQuery(&'static str),
    NotFound,
}

pub fn serve(artstation: Arc<ArtStation>) {
    local_api::serve(ARTSTATION_API_PORT, move |address| answer(&artstation, address));
}

fn answer(artstation: &ArtStation, address: &Url) -> Reply {
    match route(address) {
        Route::Help => Reply::new(200, help()),
        Route::Health => Reply::new(200, json!({ "ok": true })),
        Route::Artworks(Search { query, limit }) => search(artstation, &query, limit),
        Route::BadQuery(message) => Reply::error(400, "bad_query", message),
        Route::NotFound => Reply::not_found(),
    }
}

fn route(address: &Url) -> Route {
    match address.path() {
        "/" => Route::Help,
        "/health" => Route::Health,
        "/artworks" => local_api::search_terms(address, MISSING_QUERY).map_or_else(Route::BadQuery, Route::Artworks),
        _ => Route::NotFound,
    }
}

fn search(artstation: &ArtStation, query: &str, limit: usize) -> Reply {
    match artstation.search_artworks(query, limit) {
        Ok(artworks) => Reply::new(
            200,
            json!({
                "query": query,
                "count": artworks.len(),
                "artworks": artworks.iter().map(artwork_json).collect::<Vec<_>>(),
            }),
        ),
        Err(ArtstationError::Network(detail)) => Reply::error(502, "artstation", &detail),
        Err(other) => Reply::error(500, "internal", &other.to_string()),
    }
}

fn artwork_json(artwork: &Artwork) -> Value {
    json!({
        "id": artwork.id,
        "title": artwork.title,
        "author": artwork.author,
        "image": artwork.image,
        "image_large": artwork.sized(DETAIL_ARTWORK_SIZE),
        "link": artwork.page(),
    })
}

fn help() -> Value {
    json!({
        "name": "Noctics ArtStation API",
        "free": true,
        "address": local_api::address(ARTSTATION_API_PORT),
        "endpoints": {
            "/artworks?q=<запит>&limit=<1-100>": "Роботи з пошуку ArtStation: id, title, author, image, image_large, link",
            "/health": "Чи працює API",
        },
        "note": "Працює, поки відкрито Noctics. Вхід в ArtStation не потрібен, роботи 18+ відсіяно.",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local_api::{DEFAULT_LIMIT, local};

    #[test]
    fn routes_artwork_searches() {
        assert_eq!(
            route(&local("/artworks?q=dark+fantasy&limit=12")),
            Route::Artworks(Search {
                query: "dark fantasy".into(),
                limit: 12
            })
        );
        assert_eq!(
            route(&local("/artworks?q=castle")),
            Route::Artworks(Search {
                query: "castle".into(),
                limit: DEFAULT_LIMIT
            })
        );
        assert_eq!(route(&local("/artworks")), Route::BadQuery(MISSING_QUERY));
        assert_eq!(route(&local("/")), Route::Help);
        assert_eq!(route(&local("/health")), Route::Health);
        assert_eq!(route(&local("/pins?q=a")), Route::NotFound);
    }

    #[test]
    fn describes_artworks_with_links_and_large_images() {
        let artwork = Artwork::checked(
            "2B1Xae",
            "https://cdna.artstation.com/p/assets/images/images/085/683/878/smaller_square/sketch.jpg?1741371096",
            "Castle",
            "Martin",
        )
        .unwrap();
        let json = artwork_json(&artwork);
        assert_eq!(json["link"], "https://www.artstation.com/artwork/2B1Xae");
        assert_eq!(
            json["image_large"],
            "https://cdna.artstation.com/p/assets/images/images/085/683/878/medium/sketch.jpg?1741371096"
        );
    }
}
