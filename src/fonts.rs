use dioxus::prelude::*;

const LATIN: &str = "U+0000-00FF, U+0131, U+0152-0153, U+02BB-02BC, U+02C6, U+02DA, U+02DC, U+0304, U+0308, U+0329, U+2000-206F, U+20AC, U+2122, U+2191, U+2193, U+2212, U+2215, U+FEFF, U+FFFD";
const CYRILLIC: &str = "U+0301, U+0400-045F, U+0490-0491, U+04B0-04B1, U+2116";

pub fn font_faces() -> String {
    let faces: [(&str, &str, Asset, &str); 6] = [
        ("Unbounded", "200 900", asset!("/assets/fonts/unbounded-latin.woff2"), LATIN),
        ("Unbounded", "200 900", asset!("/assets/fonts/unbounded-cyrillic.woff2"), CYRILLIC),
        ("Manrope", "200 800", asset!("/assets/fonts/manrope-latin.woff2"), LATIN),
        ("Manrope", "200 800", asset!("/assets/fonts/manrope-cyrillic.woff2"), CYRILLIC),
        ("JetBrains Mono", "100 800", asset!("/assets/fonts/jetbrains-mono-latin.woff2"), LATIN),
        ("JetBrains Mono", "100 800", asset!("/assets/fonts/jetbrains-mono-cyrillic.woff2"), CYRILLIC),
    ];
    faces
        .iter()
        .map(|(family, weights, file, range)| {
            format!(
                "@font-face{{font-family:\"{family}\";font-style:normal;font-weight:{weights};font-display:block;src:url(\"{file}\") format(\"woff2\");unicode-range:{range};}}"
            )
        })
        .collect()
}
