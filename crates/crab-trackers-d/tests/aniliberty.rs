use crab_trackers_d::aniliberty::models::AnilibertyApiResponse;
use crab_trackers_d::aniliberty::parser;

const HOST: &str = "https://aniliberty.top";

const SAMPLE: &str = r#"{
  "data": [
    {
      "id": 1, "hash": "abc", "size": 1610612736,
      "magnet": "magnet:?xt=urn:btih:a2e092da06e84fe18b9dc5ca20bf5cc896fceaeb",
      "label": "Шоу - AniLibria.TV [WEBRip 1080p HEVC][01-12]",
      "created_at": "2024-05-01T12:00:00+00:00", "updated_at": null,
      "quality": {"value": "1080p", "description": "1080p"},
      "type": {"value": "WEBRip", "description": "WEBRip"},
      "seeders": 10, "leechers": "2",
      "release": {"id": 5, "name": {"main": "Шоу", "english": "Show"}, "year": 2024,
                  "type": {"value": "TV"}, "alias": "show"}
    },
    { "id": 2, "hash": "def", "magnet": "", "release": null },
    { "id": 3, "hash": "ghi", "magnet": "magnet:?xt=urn:btih:x", "release": {"name": {"main": " ", "english": null}} }
  ],
  "meta": {"current_page": 1, "last_page": 7, "per_page": 50, "total": 300}
}"#;

#[test]
fn maps_api_page() {
    let resp: AnilibertyApiResponse = serde_json::from_str(SAMPLE).expect("json");
    assert_eq!(resp.meta.as_ref().map(|m| m.last_page), Some(7));
    let rows = parser::map_page_torrents(&resp, HOST);
    assert_eq!(rows.len(), 1);
    let t = &rows[0];
    assert_eq!(t.trackerName, "aniliberty");
    assert_eq!(t.url, format!("{HOST}/anime/releases/release/show?hash=abc"));
    assert_eq!(t.title, "Шоу / Show / 2024 / [WEBRip 1080p HEVC][01-12]");
    assert_eq!(t.types, vec!["anime".to_string(), "serial".to_string()]);
    assert_eq!(t.sizeName, "1.50 GB");
    assert_eq!(t.quality, 1080);
    assert_eq!(t.videotype, "webrip");
    assert_eq!((t.sid, t.pir, t.relased), (10, 2, 2024));
    assert_eq!(t.createTime, t.updateTime);
}

#[test]
fn helpers() {
    assert_eq!(parser::determine_types(""), &["anime"]);
    assert_eq!(parser::determine_types("movie"), &["anime", "movie"]);
    assert_eq!(parser::determine_types("OAD"), &["anime", "ova"]);
    assert_eq!(parser::determine_types("WEB"), &["anime", "ona"]);
    assert_eq!(parser::determine_types("DORAMA"), &["dorama"]);
    assert_eq!(parser::format_size(1_048_576), "1.00 Mb");
    assert_eq!(parser::format_size(1_099_511_627_776), "1.00 TB");
    assert_eq!(parser::parse_quality(""), 480);
    assert_eq!(parser::parse_quality("4K"), 2160);
    assert_eq!(parser::parse_quality("720p"), 720);
    assert_eq!(parser::parse_quality("360p"), 360);
    assert_eq!(parser::extract_quality_info("x [a] [b]  "), "[a] [b]");
    assert_eq!(parser::extract_quality_info("no brackets"), "");
}
