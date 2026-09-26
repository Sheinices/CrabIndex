/** Helpers for the update page (pure, unit-tested). */

export const STAGES = {
  idle: 'Ожидание',
  downloading: 'Загрузка архива',
  verifying: 'Проверка контрольной суммы',
  installing: 'Установка файлов',
  restarting: 'Перезапуск службы',
  error: 'Ошибка',
}

export const BUSY_STAGES = new Set(['downloading', 'verifying', 'installing', 'restarting'])

export function stageLabel(stage) {
  return STAGES[stage] || stage || STAGES.idle
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
