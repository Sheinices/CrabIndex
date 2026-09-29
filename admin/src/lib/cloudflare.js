// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

import { tGlobal as tg } from '../lang/index.jsx'

/** Helpers for the FlareSolverr page (pure, unit-tested). */

export const ERROR_KINDS = {
  tabCrashed: { get label() { return tg('cf_kind_tabCrashed') }, get hint() { return tg('cf_kind_tabCrashed_hint') } },
  browserTimeout: { get label() { return tg('cf_kind_browserTimeout') }, get hint() { return tg('cf_kind_browserTimeout_hint') } },
  challengeFailed: { get label() { return tg('cf_kind_challengeFailed') }, get hint() { return tg('cf_kind_challengeFailed_hint') } },
  sessionError: { get label() { return tg('cf_kind_sessionError') }, get hint() { return tg('cf_kind_sessionError_hint') } },
  unreachable: { get label() { return tg('cf_kind_unreachable') }, get hint() { return tg('cf_kind_unreachable_hint') } },
  pageFailed: { get label() { return tg('cf_kind_pageFailed') }, get hint() { return tg('cf_kind_pageFailed_hint') } },
  other: { get label() { return tg('cf_kind_other') }, get hint() { return tg('cf_kind_other_hint') } },
}

export function kindLabel(kind) {
  return ERROR_KINDS[kind]?.label || kind || tg('cf_kind_other')
}

/** Average browser request time in seconds (one decimal), or null. */
export function avgSeconds(h) {
  if (!h?.browserRequests) return null
  return Math.round(h.totalBrowserMs / h.browserRequests / 100) / 10
}

/** Share of failed browser requests, 0..100 (integer), or null without requests. */
export function failRate(h) {
  if (!h?.browserRequests) return null
  return Math.round((h.browserFailed / h.browserRequests) * 100)
}

/** Sum of per-host counters. */
export function totals(hosts = []) {
  const keys = ['browserRequests', 'browserOk', 'browserFailed', 'tabCrashed', 'browserTimeouts', 'challengeFailed', 'fastOk', 'fastFailed', 'sessionsCreated']
  const t = Object.fromEntries(keys.map((k) => [k, 0]))
  for (const h of hosts) for (const k of keys) t[k] += Number(h?.[k]) || 0
  return t
}

/** 'bad' | 'warn' | 'ok' | 'idle' for a host row. */
export function hostTone(h) {
  const rate = failRate(h)
  if (rate === null) return 'idle'
  if (h.tabCrashed > 0 || rate >= 30) return 'bad'
  if (rate > 0) return 'warn'
  return 'ok'
}

/**
 * Concrete advice from the counters: what to raise or cut.
 * Returns [{ tone: 'danger'|'warn'|'info', title, text, command? }].
 */
export function advice(status) {
  const out = []
  if (!status) return out
  const hosts = status.stats?.hosts || []
  const t = totals(hosts)
  if (status.enabled && !status.paused && status.solver && status.solver.reachable === false) {
    out.push({
      tone: 'danger',
      title: tg('cf_adv_down_title'),
      text: tg('cf_adv_down_text', { url: status.settings?.url || '' }),
      command: 'docker ps -a --filter name=flaresolverr && docker logs --tail 50 flaresolverr',
    })
  }
  if (t.tabCrashed > 0) {
    const worst = hosts.filter((h) => h.tabCrashed > 0).map((h) => h.host)
    const idle = Number(status.settings?.sessionIdleMinutes)
    const idleNote = Number.isFinite(idle) && idle > 30 ? ' ' + tg('cf_adv_crash_idle', { idle }) : ''
    out.push({
      tone: 'danger',
      title: tg('cf_adv_crash_title', { n: t.tabCrashed }),
      text: tg('cf_adv_crash_text', { hosts: worst.join(', ') }) + idleNote,
      command: 'docker stats flaresolverr --no-stream && docker update --memory 6g --memory-swap 6g flaresolverr',
    })
  }
  if (t.browserTimeouts >= 3) {
    out.push({
      tone: 'warn',
      title: tg('cf_adv_timeout_title', { n: t.browserTimeouts }),
      text: tg('cf_adv_timeout_text'),
      command: 'docker update --cpus 2 flaresolverr',
    })
  }
  if (t.fastFailed > t.fastOk && t.fastFailed >= 10) {
    out.push({
      tone: 'info',
      title: tg('cf_adv_fast_title'),
      text: tg('cf_adv_fast_text'),
    })
  }
  return out
}
