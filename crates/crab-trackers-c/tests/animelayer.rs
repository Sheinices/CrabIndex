use crab_trackers_c::animelayer::parser::parse_torrent_list_from_html;

const HOST: &str = "https://animelayer.ru";

#[test]
fn parse_torrent_list_from_html_extracts_listing_size() {
    let html = r#"
        <div class="torrent-item torrent-item-medium panel">
            <h3 class="h2 m0">
                <a href="/torrent/6a4a7093aa0a9cff9d0ddb83/">Kimi no Koto ga Dai Dai Dai Dai Daisuki na Hyakunin no Kanojo (2026) / Сто девушек [ТВ-3] (1-6)</a>
            </h3>
            <div class="info pd20">
                <i class="icon s-icons-upload"></i>&nbsp;1
                <span class="gray">&nbsp;|&nbsp;</span>
                <i class="icon s-icons-download"></i>&nbsp;2
                <span class="gray">&nbsp;|&nbsp;</span>
                2.19 GB
                <span class="gray">&nbsp;|&nbsp;</span>
                <span class="gray">Обновлён:</span>&nbsp;9 августа в&nbsp;18:05
            </div>
            <strong>Год выхода: </strong>2026
        </div>
    "#;

    let torrents = parse_torrent_list_from_html(html, HOST, 1);
    assert_eq!(torrents.len(), 1);
    let t = &torrents[0];
    assert_eq!(t.url, format!("{HOST}/torrent/6a4a7093aa0a9cff9d0ddb83/"));
    assert_eq!(t.sizeName, "2.19 GB");
    assert_eq!(t.sid, 1);
    assert_eq!(t.pir, 2);
    assert!(!crab_core::time::is_min(&t.createTime));
    assert_eq!(t.name, "Сто девушек");
    assert_eq!(t.originalname, "Kimi no Koto ga Dai Dai Dai Dai Daisuki na Hyakunin no Kanojo");
    assert_eq!(t.relased, 2026);
    assert_eq!(t.types, vec!["anime"]);
}
