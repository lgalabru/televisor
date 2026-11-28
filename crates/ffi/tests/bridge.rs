//! The bridge end to end, through its JSON entry point, against a fake panel.
//!
//! The bridge holds one global session, so the whole flow runs in a single
//! test, in order, instead of in parallel tests that would race on it.

use serde_json::{Value, json};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/src/xtream/fixtures");

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("{FIXTURES}/{name}")).expect("fixture exists")
}

fn call(request: Value) -> Value {
    let response = televisor_ffi::call(request.to_string().as_bytes());
    serde_json::from_str(&response).expect("responses are JSON")
}

fn ok(request: Value) -> Value {
    let response = call(request.clone());
    match response.get("ok") {
        Some(value) => value.clone(),
        None => panic!("{request} failed: {response}"),
    }
}

fn error_kind(request: Value) -> String {
    let response = call(request.clone());
    match response.pointer("/error/kind").and_then(Value::as_str) {
        Some(kind) => kind.to_string(),
        None => panic!("{request} should have failed: {response}"),
    }
}

fn mock_action(server: &mut mockito::Server, action: &str, body: String) -> mockito::Mock {
    server
        .mock("GET", "/player_api.php")
        .match_query(mockito::Matcher::UrlEncoded("action".into(), action.into()))
        .with_body(body)
        .create()
}

#[test]
fn bridge_flow_against_a_fake_panel() {
    let mut server = mockito::Server::new();
    let dir = tempfile::tempdir().unwrap();
    let open = |host: String, password: &str| {
        json!({"method": "open", "params": {
            "account_name": "home",
            "host": host,
            "username": "alice",
            "password": password,
            "cache_dir": dir.path().join("cache"),
            "state_path": dir.path().join("state.toml"),
        }})
    };

    // Malformed and oversized requests are refused before anything runs.
    assert_eq!(error_kind(json!({"method": "nope"})), "invalid_request");
    let oversized = vec![b' '; televisor_ffi::protocol::REQUEST_BYTES_MAX + 1];
    let response: Value =
        serde_json::from_str(&televisor_ffi::call(&oversized)).expect("responses are JSON");
    assert_eq!(
        response.pointer("/error/kind"),
        Some(&json!("invalid_request"))
    );

    // Nothing works before sign-in.
    assert_eq!(error_kind(json!({"method": "favorites"})), "not_signed_in");

    // A rejected login is an auth failure and leaves no session behind.
    let rejected = server
        .mock("GET", "/player_api.php")
        .match_query(mockito::Matcher::UrlEncoded(
            "password".into(),
            "wrong".into(),
        ))
        .with_body(fixture("user_info_auth_failed.json"))
        .create();
    assert_eq!(error_kind(open(server.url(), "wrong")), "auth_failed");
    assert_eq!(error_kind(json!({"method": "history"})), "not_signed_in");
    rejected.remove();

    let account = server
        .mock("GET", "/player_api.php")
        .match_query(mockito::Matcher::UrlEncoded(
            "password".into(),
            "hunter2".into(),
        ))
        .with_body(fixture("user_info.json"))
        .create();
    let overview = ok(open(server.url(), "hunter2"));
    assert_eq!(overview["status"], "Active");
    assert_eq!(overview["connections_max"], 1);
    account.remove();

    // Live channels: listed in panel order, logos carried through, adult
    // channels never listed, the list fetched once and then memoized.
    let streams = mock_action(
        &mut server,
        "get_live_streams",
        fixture("live_streams.json"),
    )
    .expect(1);
    let page = ok(json!({"method": "browse", "params": {"library": "live", "limit": 200}}));
    let items = page["items"].as_array().unwrap();
    assert!(!items.is_empty());
    assert_eq!(page["total"].as_u64().unwrap(), items.len() as u64);
    // The fixture opens with a "### WORLD CUP 2026 ###" heading row, which
    // plays nothing and is left out.
    assert!(
        items
            .iter()
            .all(|item| !item["name"].as_str().unwrap().starts_with('#'))
    );
    assert_eq!(items[0]["image"], "https://img.example/fs1.jpg");
    // Paging past the end is empty, and still reports the total.
    let paged = ok(json!({"method": "browse", "params": {
        "library": "live", "offset": 1, "limit": 1,
    }}));
    assert!(paged["items"].as_array().unwrap().is_empty());
    assert_eq!(paged["total"], 1);
    let searched = ok(json!({"method": "browse", "params": {
        "library": "live", "query": "fox sports", "limit": 10,
    }}));
    assert!(
        searched["items"][0]["name"]
            .as_str()
            .unwrap()
            .contains("FOX SPORTS")
    );
    streams.assert();

    // Now and next, batched; an oversized batch is refused.
    let _epg = mock_action(&mut server, "get_short_epg", fixture("short_epg.json"));
    let batch = ok(json!({"method": "epg_now", "params": {"stream_ids": [2012600, 2012667]}}));
    assert_eq!(batch.as_array().unwrap().len(), 2);
    assert_eq!(batch[0]["stream_id"], 2012600);
    let too_many: Vec<u64> = (0..=televisor_ffi::protocol::EPG_BATCH_MAX as u64).collect();
    assert_eq!(
        error_kind(json!({"method": "epg_now", "params": {"stream_ids": too_many}})),
        "invalid_request"
    );

    // Series detail: episodes grouped by season, in order.
    let _series = mock_action(&mut server, "get_series_info", fixture("series_info.json"));
    let detail = ok(json!({"method": "series", "params": {"id": 1}}));
    assert_eq!(detail["poster"], "https://img.example/cover.jpg");
    let seasons = detail["seasons"].as_array().unwrap();
    assert_eq!(seasons[0]["number"], 1);
    assert_eq!(seasons[1]["number"], 2);
    assert_eq!(seasons[0]["episodes"][0]["duration_secs"], 2221);

    // Play: an HLS URL for live, and the play lands in history.
    let playback = ok(json!({"method": "play", "params": {
        "kind": "live", "id": 2012600, "name": "US: FOX SPORTS 1",
        "image": "https://img.example/fs1.jpg",
    }}));
    assert_eq!(playback["is_live"], true);
    assert!(playback["url"].as_str().unwrap().ends_with("/2012600.m3u8"));
    assert_eq!(
        error_kind(json!({"method": "play", "params": {"kind": "live", "id": 1, "name": "x"}})),
        "not_found"
    );
    let history = ok(json!({"method": "history"}));
    assert_eq!(history[0]["id"], 2012600);
    assert_eq!(history[0]["image"], "https://img.example/fs1.jpg");

    // Favorites toggle on and off.
    let favorite = |is_favorite: bool| {
        json!({"method": "set_favorite", "params": {
            "kind": "live", "id": 2012600, "name": "FS1", "is_favorite": is_favorite,
        }})
    };
    assert_eq!(ok(favorite(true)).as_array().unwrap().len(), 1);
    assert_eq!(ok(favorite(true)).as_array().unwrap().len(), 1);
    assert!(ok(favorite(false)).as_array().unwrap().is_empty());

    // Old movie favorites did not store their container extension. Playing
    // one repairs it from the catalog instead of silently constructing an
    // incorrect .mp4 URL; the repaired value also lands in history.
    let movies = mock_action(&mut server, "get_vod_streams", fixture("vod_streams.json")).expect(1);
    let playback = ok(json!({"method": "play", "params": {
        "kind": "movie", "id": 2024998, "name": "4K-TOP - Michael (2026)",
    }}));
    assert!(playback["url"].as_str().unwrap().ends_with("/2024998.mkv"));
    let history = ok(json!({"method": "history"}));
    assert_eq!(history[0]["ext"], "mkv");
    movies.assert();

    // Shows are bookmarked like favorites, and are never playable.
    let shows = ok(json!({"method": "set_favorite", "params": {
        "kind": "series", "id": 48173, "name": "Breaking Bad", "is_favorite": true,
    }}));
    assert_eq!(shows[0]["kind"], "series");
    assert_eq!(
        error_kind(
            json!({"method": "play", "params": {"kind": "series", "id": 48173, "name": "x"}})
        ),
        "invalid_request"
    );

    // Closing ends the session.
    ok(json!({"method": "close"}));
    assert_eq!(error_kind(json!({"method": "history"})), "not_signed_in");
}

/// Remuxing needs no session; its failures tell the app to use VLC.
#[test]
fn remux_open_reports_what_the_app_falls_back_on() {
    assert_eq!(
        error_kind(json!({"method": "remux_open", "params": {"url": "not a url"}})),
        "invalid_request"
    );
    assert_eq!(
        error_kind(json!({"method": "remux_open", "params": {"url": "file:///etc/hosts"}})),
        "invalid_request"
    );

    let mut server = mockito::Server::new();
    let _avi = server
        .mock("GET", "/movie/alice/hunter2/7.mkv")
        .with_status(206)
        .with_header("content-range", "bytes 0-15/16")
        .with_body("RIFF....AVI LIST")
        .create();
    let url = format!("{}/movie/alice/hunter2/7.mkv", server.url());
    assert_eq!(
        error_kind(json!({"method": "remux_open", "params": {"url": url}})),
        "unsupported"
    );

    let _down = server
        .mock("GET", "/movie/alice/hunter2/8.mkv")
        .with_status(503)
        .create();
    let url = format!("{}/movie/alice/hunter2/8.mkv", server.url());
    assert_eq!(
        error_kind(json!({"method": "remux_open", "params": {"url": url}})),
        "network"
    );

    let stale =
        json!({"method": "remux_close", "params": {"url": "http://127.0.0.1:1/x/index.m3u8"}});
    assert_eq!(ok(stale), Value::Null);
}
