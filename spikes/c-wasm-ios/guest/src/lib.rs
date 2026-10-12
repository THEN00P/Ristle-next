//! Plugin-shaped workloads, shared by the WASM guest and the native baseline.

use aes::cipher::{KeyIvInit, StreamCipher};
use serde::Deserialize;

pub struct Track {
    pub id: String,
    pub title: String,
    pub artist: String,
    pub duration_ms: u32,
    pub artwork: Option<String>,
}

#[derive(Deserialize)]
struct Page {
    collection: Vec<RawTrack>,
}

#[derive(Deserialize)]
struct RawTrack {
    id: u64,
    title: String,
    duration: u32,
    artwork_url: Option<String>,
    user: RawUser,
    #[allow(dead_code)]
    #[serde(default)]
    media: serde_json::Value,
}

#[derive(Deserialize)]
struct RawUser {
    username: String,
}

pub fn search_pages(query: &str, pages: impl Iterator<Item = Vec<u8>>) -> Vec<Track> {
    let needle = query.to_lowercase();
    let mut out = Vec::new();
    for bytes in pages {
        let page: Page = serde_json::from_slice(&bytes).expect("page json");
        for raw in page.collection {
            if raw.title.to_lowercase().contains(&needle) || raw.user.username.to_lowercase().contains(&needle) {
                out.push(Track {
                    id: raw.id.to_string(),
                    title: raw.title,
                    artist: raw.user.username,
                    duration_ms: raw.duration,
                    artwork: raw.artwork_url,
                });
            }
        }
    }
    out
}

pub fn decrypt(key: &[u8], iv: &[u8], data: &mut [u8]) {
    let mut cipher = ctr::Ctr128BE::<aes::Aes128>::new(key.into(), iv.into());
    cipher.apply_keystream(data);
}

/// Deterministic synthetic page: 50 SoundCloud-shaped tracks with nested media.
pub fn synthetic_page(page: u32) -> Vec<u8> {
    let mut s = String::from("{\"collection\":[");
    for i in 0..50u32 {
        let n = page * 50 + i;
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!(
            "{{\"id\":{id},\"title\":\"Track {n} {word}\",\"duration\":{dur},\"artwork_url\":\"https://i1.sndcdn.com/artworks-{id}-large.jpg\",\"user\":{{\"username\":\"artist{u}\",\"permalink_url\":\"https://soundcloud.com/artist{u}\"}},\"media\":{{\"transcodings\":[{{\"url\":\"https://api-v2.soundcloud.com/media/soundcloud:tracks:{id}/stream/hls\",\"format\":{{\"protocol\":\"hls\",\"mime_type\":\"audio/mp4; codecs=\\\"mp4a.40.2\\\"\"}},\"quality\":\"sq\",\"preset\":\"aac_160k\"}},{{\"url\":\"https://api-v2.soundcloud.com/media/soundcloud:tracks:{id}/stream/progressive\",\"format\":{{\"protocol\":\"progressive\",\"mime_type\":\"audio/mpeg\"}},\"quality\":\"sq\",\"preset\":\"mp3_1_0\"}}]}},\"genre\":\"Electronic\",\"tag_list\":\"house techno deep\",\"playback_count\":{pc},\"likes_count\":{lc}}}",
            id = 1_000_000 + n,
            word = if n % 3 == 0 { "daft" } else { "other" },
            dur = 180_000 + n * 7,
            u = n % 97,
            pc = n * 13,
            lc = n * 3,
        ));
    }
    s.push_str("]}");
    s.into_bytes()
}

#[cfg(target_arch = "wasm32")]
mod component {
    wit_bindgen::generate!({ path: "../wit", world: "plugin" });

    use exports::ristle::spike::provider::{Guest, Track};

    struct Plugin;

    impl Guest for Plugin {
        fn search(query: String, pages: u32) -> Vec<Track> {
            let iter = (0..pages).map(ristle::spike::host::fetch_page);
            super::search_pages(&query, iter)
                .into_iter()
                .map(|t| Track { id: t.id, title: t.title, artist: t.artist, duration_ms: t.duration_ms, artwork: t.artwork })
                .collect()
        }

        fn decrypt(key: Vec<u8>, iv: Vec<u8>, mut data: Vec<u8>) -> Vec<u8> {
            super::decrypt(&key, &iv, &mut data);
            data
        }
    }

    export!(Plugin);
}
