use std::io::Read;
use std::thread;
use std::time::Duration;

use noctics_shared::{AssetFind, is_image_link};
use url::Url;

const PAGE_TIMEOUT: Duration = Duration::from_secs(12);
const MAX_PAGE_BYTES: u64 = 768 * 1024;
const PARALLEL_PAGES: usize = 6;
const USER_AGENT: &str = "Mozilla/5.0 (compatible; NocticsFactory/0.1)";
const ACCEPT_HTML: &str = "text/html,application/xhtml+xml";
const META_OPENING: &str = "<meta";
const IMAGE_PROPERTIES: [&str; 5] = [
    "og:image:secure_url",
    "og:image",
    "og:image:url",
    "twitter:image",
    "twitter:image:src",
];
const ENTITIES: [(&str, &str); 6] = [
    ("&quot;", "\""),
    ("&#39;", "'"),
    ("&#x27;", "'"),
    ("&lt;", "<"),
    ("&gt;", ">"),
    ("&amp;", "&"),
];

pub fn attach(finds: &mut [AssetFind]) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(PAGE_TIMEOUT))
        .http_status_as_error(false)
        .build()
        .into();
    for batch in finds.chunks_mut(PARALLEL_PAGES) {
        thread::scope(|scope| {
            for find in batch.iter_mut().filter(|find| find.image.is_none()) {
                let agent = &agent;
                scope.spawn(move || find.image = preview_of(agent, &find.url));
            }
        });
    }
}

fn preview_of(agent: &ureq::Agent, page: &str) -> Option<String> {
    let mut response = agent
        .get(page)
        .header("User-Agent", USER_AGENT)
        .header("Accept", ACCEPT_HTML)
        .call()
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(MAX_PAGE_BYTES)
        .read_to_end(&mut bytes)
        .ok()?;
    let html = String::from_utf8_lossy(&bytes);
    let found = meta_image(&html)?;
    let link = Url::parse(page).ok()?.join(&decode_entities(&found)).ok()?.to_string();
    is_image_link(&link).then_some(link)
}

fn meta_image(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut best: Option<(usize, String)> = None;
    let mut cursor = 0;
    while let Some(offset) = lower[cursor..].find(META_OPENING) {
        let start = cursor + offset;
        let end = lower[start..].find('>').map_or(lower.len(), |close| start + close);
        let attributes = attributes_of(&html[start + META_OPENING.len()..end]);
        let key = value_of(&attributes, "property").or_else(|| value_of(&attributes, "name"));
        let content = value_of(&attributes, "content").map(str::trim).filter(|text| !text.is_empty());
        let rank = key.and_then(|key| IMAGE_PROPERTIES.iter().position(|wanted| key.eq_ignore_ascii_case(wanted)));
        if let (Some(rank), Some(content)) = (rank, content)
            && best.as_ref().is_none_or(|(kept, _)| rank < *kept)
        {
            best = Some((rank, content.to_owned()));
        }
        cursor = end;
    }
    best.map(|(_, content)| content)
}

fn attributes_of(tag: &str) -> Vec<(String, String)> {
    let bytes = tag.as_bytes();
    let skip_space = |mut index: usize| {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b'/') {
            index += 1;
        }
        index
    };
    let mut found = Vec::new();
    let mut index = skip_space(0);
    while index < bytes.len() {
        let name_start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() && !matches!(bytes[index], b'=' | b'/') {
            index += 1;
        }
        let name = tag[name_start..index].to_ascii_lowercase();
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= bytes.len() || bytes[index] != b'=' {
            found.push((name, String::new()));
            index = skip_space(index);
            continue;
        }
        index += 1;
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let value = match bytes.get(index) {
            Some(&quote) if quote == b'"' || quote == b'\'' => {
                let value_start = index + 1;
                let value_end = tag[value_start..]
                    .find(quote as char)
                    .map_or(bytes.len(), |close| value_start + close);
                index = (value_end + 1).min(bytes.len());
                &tag[value_start..value_end]
            }
            _ => {
                let value_start = index;
                while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
                    index += 1;
                }
                &tag[value_start..index]
            }
        };
        found.push((name, value.to_owned()));
        index = skip_space(index);
    }
    found
}

fn value_of<'a>(attributes: &'a [(String, String)], name: &str) -> Option<&'a str> {
    attributes
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

fn decode_entities(text: &str) -> String {
    ENTITIES
        .iter()
        .fold(text.to_owned(), |decoded, (entity, plain)| decoded.replace(entity, plain))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_preview_images_in_any_attribute_order() {
        let itch = r#"<head><meta content="itch.io" property="og:site_name"/>
<meta content="https://img.itch.zone/aW1nLzIx.gif/original/tmiy6d.gif" property="og:image"/></head>"#;
        assert_eq!(
            meta_image(itch).as_deref(),
            Some("https://img.itch.zone/aW1nLzIx.gif/original/tmiy6d.gif")
        );

        let kenney = "<META property='og:image'\tcontent='/media/sample.png' />";
        assert_eq!(meta_image(kenney).as_deref(), Some("/media/sample.png"));

        let ranked = r#"<meta name="twitter:image" content="https://a.example/t.png">
<meta property=og:image content=https://a.example/o.png>"#;
        assert_eq!(meta_image(ranked).as_deref(), Some("https://a.example/o.png"));

        assert_eq!(meta_image("<meta property=\"og:image\" content=\"\">"), None);
        assert_eq!(meta_image("<p>Немає прев’ю — лише текст</p>"), None);
    }

    #[test]
    fn decodes_entities_once() {
        assert_eq!(decode_entities("a.png?x=1&amp;y=2"), "a.png?x=1&y=2");
        assert_eq!(decode_entities("&amp;lt;"), "&lt;");
    }
}
