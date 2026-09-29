// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

// Registry of built-in admin-panel languages. Each entry is compiled into the bundle and
// its dictionary lives in `./<code>.js`. Users can add more languages at runtime WITHOUT a
// rebuild by dropping `Data/lang/<code>.json` on the server (see docs) - those packs are
// fetched from `GET /api/lang` and merged on top of these at startup.
//
// `ru` is the base language: every key is defined there, and any string missing from another
// language (built-in or runtime pack) falls back to the Russian text.
export default {
  ru: { code: 'ru', name: 'Русский' },
  en: { code: 'en', name: 'English' },
}
