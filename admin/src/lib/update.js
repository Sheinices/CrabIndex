// SPDX-License-Identifier: AGPL-3.0-or-later
// Copyright (c) 2026 CrabIndex contributors

/** Helpers for the update page (pure, unit-tested). */

import { tGlobal as tg } from '../lang/index.jsx'

export const BUSY_STAGES = new Set(['downloading', 'verifying', 'installing', 'restarting'])

/** Stage name in the current language (unknown stages are shown as is). */
export function stageLabel(stage) {
  if (stage && !STAGE_KEYS[stage]) return stage
  return tg(stageKey(stage))
}

// i18n keys for each stage (see lang/*.js).
export const STAGE_KEYS = {
  idle: 'stage_idle',
  downloading: 'stage_downloading',
  verifying: 'stage_verifying',
  installing: 'stage_installing',
  restarting: 'stage_restarting',
  error: 'error',
}

export function stageKey(stage) {
  return STAGE_KEYS[stage] || STAGE_KEYS.idle
}

export function isBusy(state) {
  return BUSY_STAGES.has(state?.stage)
}

/** Version without the `+sha` suffix and a leading `v`. */
export function cleanVersion(v) {
  return String(v || '')
    .replace(/^v/, '')
    .split('+')[0]
}

/** True once the server reports the target version (after the restart). */
export function reachedTarget(sessionVersion, target) {
  return Boolean(target) && cleanVersion(sessionVersion) === cleanVersion(target)
}

/** Release notes as plain lines: markdown headers / bullets kept readable, max `limit` lines. */
export function notesLines(notes, limit = 40) {
  const lines = String(notes || '')
    .replace(/\r/g, '')
    .split('\n')
    .map((l) => l.replace(/^#{1,6}\s*/, '').replace(/\*\*(.+?)\*\*/g, '$1'))
  while (lines.length && !lines[lines.length - 1].trim()) lines.pop()
  return lines.slice(0, limit)
}
