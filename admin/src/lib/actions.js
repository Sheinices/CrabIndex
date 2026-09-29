// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { tGlobal as tg } from '../lang/index.jsx'

/**
 * Cron actions supported per tracker (`{base}/api/cron/{slug}/{action}`).
 * Built from the tracker routers in `crates/crab-trackers-*` - keep in sync.
 */

const PAGE = { name: 'page', get label() { return tg('act_p_page') }, type: 'number', get placeholder() { return tg('act_p_page_ph') } }
const PAGES = (def) => ({ name: 'pages', get label() { return tg('act_p_pages') }, type: 'number', placeholder: String(def) })
const LIMIT_PAGE = { name: 'limit_page', get label() { return tg('act_p_limit_page') }, type: 'number', get placeholder() { return tg('act_p_limit_page_ph') } }
const RANGE = [
  { name: 'parsefrom', get label() { return tg('act_p_parsefrom') }, type: 'number', get placeholder() { return tg('act_p_parsefrom_ph') } },
  { name: 'parseto', get label() { return tg('act_p_parseto') }, type: 'number', get placeholder() { return tg('act_p_parseto_ph') } },
]

export const ACTIONS = {
  parse: { get label() { return tg('act_parse') }, get description() { return tg('act_parse_desc') }, params: [] },
  updatetasksparse: {
    get label() { return tg('act_updatetasks') },
    get description() { return tg('act_updatetasks_desc') },
    params: [],
  },
  parsealltask: {
    label: 'ParseAll',
    get description() { return tg('act_parseall_desc') },
    params: [],
    heavy: true,
  },
  parselatest: {
    get label() { return tg('act_parselatest') },
    get description() { return tg('act_parselatest_desc') },
    params: [PAGES(5)],
  },
  backfill: { label: 'Backfill', get description() { return tg('act_backfill_desc') }, params: [PAGES(20)], heavy: true },
  backfillstatus: { get label() { return tg('act_backfillstatus') }, get description() { return tg('act_backfillstatus_desc') }, params: [], readOnly: true },
  parsefromdate: {
    get label() { return tg('act_parsefromdate') },
    get description() { return tg('act_parsefromdate_desc') },
    params: [
      { name: 'lastnewtor', get label() { return tg('act_p_lastnewtor') }, type: 'text', get placeholder() { return tg('act_p_lastnewtor_ph') } },
      PAGES(20),
    ],
    heavy: true,
  },
  parsepages: {
    get label() { return tg('act_parsepages') },
    get description() { return tg('act_parsepages_desc') },
    params: [
      { name: 'pagefrom', get label() { return tg('act_p_pagefrom') }, type: 'number', get placeholder() { return tg('act_p_pagefrom_ph') } },
      { name: 'pageto', get label() { return tg('act_p_pageto') }, type: 'number', get placeholder() { return tg('act_p_pageto_ph') } },
    ],
  },
  parseseasonpacks: {
    get label() { return tg('act_seasonpacks') },
    get description() { return tg('act_seasonpacks_desc') },
    params: [{ name: 'series', get label() { return tg('act_p_series') }, type: 'text', get placeholder() { return tg('act_p_series_ph') } }],
    heavy: true,
  },
  verifypage: {
    get label() { return tg('act_verifypage') },
    get description() { return tg('act_verifypage_desc') },
    params: [{ name: 'series', get label() { return tg('act_p_series') }, type: 'text', placeholder: 'The_Bear' }],
    readOnly: true,
  },
  stats: { get label() { return tg('act_stats') }, get description() { return tg('act_stats_desc') }, params: [], readOnly: true },
  parseshows: { get label() { return tg('act_parseshows') }, get description() { return tg('act_parseshows_desc') }, params: [], heavy: true },
  parseshowstatus: { get label() { return tg('act_parseshowstatus') }, get description() { return tg('act_parseshowstatus_desc') }, params: [], readOnly: true },
  takelogin: { get label() { return tg('act_takelogin') }, get description() { return tg('act_takelogin_desc') }, params: [] },
}

const FULL = ['parse', 'updatetasksparse', 'parsealltask', 'parselatest']
const PARSE_ONLY = ['parse']

export const TRACKER_ACTIONS = {
  anibelka: FULL,
  anidub: PARSE_ONLY,
  anifilm: PARSE_ONLY,
  aniliberty: PARSE_ONLY,
  animelayer: ['parse', 'takelogin'],
  anistar: PARSE_ONLY,
  baibako: PARSE_ONLY,
  bitru: ['parse', 'backfill', 'parsefromdate'],
  kinozal: FULL,
  knaben: ['parse', 'backfill', 'backfillstatus'],
  korsars: FULL,
  leproduction: PARSE_ONLY,
  lostfilm: ['parse', 'parsepages', 'parseseasonpacks', 'verifypage', 'stats'],
  mazepa: PARSE_ONLY,
  megapeer: FULL,
  nnmclub: FULL,
  rudub: PARSE_ONLY,
  rutor: FULL,
  rutracker: FULL,
  selezen: PARSE_ONLY,
  subsplease: ['parse', 'parseshows', 'parseshowstatus'],
  toloka: FULL,
  torrentby: FULL,
  ultradox: FULL,
  viruseproject: PARSE_ONLY,
}

/** Per-tracker parameter overrides (e.g. parse takes a page range). */
const PARAM_OVERRIDES = {
  'animelayer:parse': RANGE,
  'selezen:parse': RANGE,
  'aniliberty:parse': RANGE,
  'anidub:parse': RANGE,
  'baibako:parse': RANGE,
  'rudub:parse': RANGE,
  'anistar:parse': [LIMIT_PAGE],
  'leproduction:parse': [LIMIT_PAGE],
  'viruseproject:parse': [LIMIT_PAGE],
  'anifilm:parse': [{ name: 'fullparse', get label() { return tg('act_p_fullparse') }, type: 'text', placeholder: 'false' }],
  'knaben:parse': [PAGES(1)],
  'knaben:backfill': [PAGES(10)],
  'subsplease:parse': [PAGES(1)],
  'rutracker:parsealltask': [
    { name: 'cat', get label() { return tg('act_p_cat') }, type: 'text', get placeholder() { return tg('act_p_cat_ph') } },
    { name: 'maxpages', get label() { return tg('act_p_maxpages') }, type: 'number', get placeholder() { return tg('act_p_maxpages_ph') } },
  ],
}

const PAGE_PARSE = new Set(['anibelka', 'kinozal', 'korsars', 'megapeer', 'nnmclub', 'rutor', 'rutracker', 'toloka', 'torrentby', 'ultradox'])

/** Actions a tracker supports, as `[{ id, label, description, params, heavy, readOnly }]`. */
export function actionsFor(slug) {
  const ids = TRACKER_ACTIONS[String(slug || '').toLowerCase()] || []
  return ids.map((id) => {
    const def = ACTIONS[id]
    const key = `${slug}:${id}`
    let params = PARAM_OVERRIDES[key] || def.params
    if (id === 'parse' && PAGE_PARSE.has(slug) && !PARAM_OVERRIDES[key]) params = [PAGE]
    return { id, ...def, params }
  })
}

export function supportsAction(slug, action) {
  return (TRACKER_ACTIONS[String(slug || '').toLowerCase()] || []).includes(action)
}

/** Only ParseAll-capable trackers show the ParseAll column. */
export function hasParseAll(slug) {
  return supportsAction(slug, 'parsealltask')
}

/** Drop empty param values so server defaults apply. */
export function cleanParams(values) {
  const out = {}
  for (const [k, v] of Object.entries(values || {})) {
    const s = String(v ?? '').trim()
    if (s) out[k] = s
  }
  return out
}

export function trackerLogName(slug) {
  return `${String(slug).toLowerCase()}.log`
}
