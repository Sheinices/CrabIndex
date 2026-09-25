//! Normalized indexer search request shared by Jackett, Torznab and Prowlarr endpoints.

#[derive(Clone, Debug, PartialEq)]
pub struct IndexerSearchRequest {
    pub query: Option<String>,
    pub title: Option<String>,
    pub title_original: Option<String>,
    pub year: i32,
    /// -1 unknown, 0 other, 1 movie, 2 serial, 3 tvshow, 4 documentary, 5 anime.
    pub is_serial: i32,
    pub genres: Option<String>,
    pub categories: Vec<i32>,
    pub season: Option<i32>,
    pub episode: Option<i32>,
    pub tracker: Option<String>,
    pub trackers: Vec<String>,
    pub card_mode: bool,
    pub api_key: Option<String>,
    pub rq_num: bool,
}

impl Default for IndexerSearchRequest {
    fn default() -> Self {
        IndexerSearchRequest {
            query: None,
            title: None,
            title_original: None,
            year: 0,
            is_serial: -1,
            genres: None,
            categories: Vec::new(),
            season: None,
            episode: None,
            tracker: None,
            trackers: Vec::new(),
            card_mode: false,
            api_key: None,
            rq_num: false,
        }
    }
}
