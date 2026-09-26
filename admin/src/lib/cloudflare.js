/** Helpers for the FlareSolverr page (pure, unit-tested). */

export const ERROR_KINDS = {
  tabCrashed: { label: 'Вкладка упала', hint: 'браузеру не хватило памяти' },
  browserTimeout: { label: 'Таймаут браузера', hint: 'не хватает CPU или сайт отвечает медленно' },
  challengeFailed: { label: 'Проверка не пройдена', hint: 'Cloudflare не пропустил браузер' },
  sessionError: { label: 'Ошибка сессии', hint: 'сессия браузера потеряна' },
  unreachable: { label: 'Нет связи', hint: 'FlareSolverr не отвечает' },
  pageFailed: { label: 'Страница не подошла', hint: 'не 200 или заглушка вместо страницы' },
  other: { label: 'Другое', hint: '' },
}

export function kindLabel(kind) {
  return ERROR_KINDS[kind]?.label || kind || 'Другое'
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
      title: 'FlareSolverr не отвечает',
      text: `Адрес ${status.settings?.url || ''} недоступен: закрытые Cloudflare трекеры не парсятся. Проверьте контейнер.`,
      command: 'docker ps -a --filter name=flaresolverr && docker logs --tail 50 flaresolverr',
    })
  }
  if (t.tabCrashed > 0) {
    const worst = hosts.filter((h) => h.tabCrashed > 0).map((h) => h.host)
    out.push({
      tone: 'danger',
      title: `Вкладки браузера падают по памяти: ${t.tabCrashed}`,
      text: `Контейнеру FlareSolverr не хватает памяти (${worst.join(', ')}). Поднимите лимит, это можно сделать без перезапуска. Или закройте сессии, чтобы освободить память сейчас.`,
      command: 'docker update --memory 4g --memory-swap 4g flaresolverr',
    })
  }
  if (t.browserTimeouts >= 3) {
    out.push({
      tone: 'warn',
      title: `Таймауты браузера: ${t.browserTimeouts}`,
      text: 'Браузер не успевает пройти проверку. Обычно не хватает процессора: поднимите лимит CPU контейнера или уменьшите число трекеров за Cloudflare.',
      command: 'docker update --cpus 2 flaresolverr',
    })
  }
  if (t.fastFailed > t.fastOk && t.fastFailed >= 10) {
    out.push({
      tone: 'info',
      title: 'Быстрый путь (cffetch) чаще не срабатывает',
      text: 'Страницы идут через браузер, это медленнее и тяжелее. Проверьте, что cffetch запущен и что у него нет лишнего прокси (cffetch.proxy).',
    })
  }
  return out
}
