// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

//! FileDB-backed search services (Jackett card search, native torrents/qualitys, tracker list).

pub mod card_matcher;
pub mod jackett_service;
pub mod result_builder;
pub mod torrent_query;
pub mod tracker_catalog;
