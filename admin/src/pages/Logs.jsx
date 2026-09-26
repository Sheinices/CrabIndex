import { useEffect, useMemo, useRef, useState } from 'react'
import { useSearchParams } from 'react-router'
import { ArrowDownToLine, FileText, RefreshCw, Search } from 'lucide-react'
import { getLog, getLogs } from '../lib/api.js'
import { FdbJournalCard } from './FdbJournal.jsx'
import { usePolling } from '../hooks/usePolling.js'
import { ErrorBox, PageHeader, Spinner, Toggle } from '../components/ui.jsx'
import { formatBytes, formatDate } from '../lib/format.js'
import { highlight, levelClass } from '../lib/highlight.jsx'

const LINE_OPTIONS = [100, 300, 1000, 3000]

function LogViewer({ name }) {
  const [lines, setLines] = useState(300)
  const [live, setLive] = useState(false)
  const [query, setQuery] = useState('')
  const [onlyMatches, setOnlyMatches] = useState(false)
  const [follow, setFollow] = useState(true)
  const boxRef = useRef(null)
  const { data, error, loading, reload } = usePolling(() => getLog(name, lines), live ? 5000 : 0, { enabled: live })

  useEffect(() => {
    reload()
  }, [name, lines, reload])

  const all = useMemo(() => (Array.isArray(data?.lines) ? data.lines : []), [data])
  const needle = query.trim().toLowerCase()
  const shown = useMemo(() => (onlyMatches && needle ? all.filter((l) => l.toLowerCase().includes(needle)) : all), [all, onlyMatches, needle])
  const matches = useMemo(() => (needle ? all.filter((l) => l.toLowerCase().includes(needle)).length : 0), [all, needle])

  useEffect(() => {
    if (follow && boxRef.current) boxRef.current.scrollTop = boxRef.current.scrollHeight
  }, [shown, follow])

  return (
    <div className="flex min-h-0 flex-col gap-3">
      <div className="flex flex-wrap items-center gap-3">
        <h2 className="font-mono text-sm font-semibold">{name}</h2>
        <label className="ml-auto flex items-center gap-2 text-sm">
          <span className="text-muted">Строк</span>
          <select className="input w-auto py-1.5" value={lines} onChange={(e) => setLines(Number(e.target.value))}>
            {LINE_OPTIONS.map((n) => (
              <option key={n} value={n}>
                {n}
              </option>
            ))}
          </select>
        </label>
        <Toggle id="log-live" checked={live} onChange={setLive} label="Автообновление" />
        <button type="button" className="btn btn-sm" onClick={reload} aria-label="Обновить лог">
          <RefreshCw className={`size-4 ${loading ? 'animate-spin' : ''}`} aria-hidden="true" />
        </button>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <div className="relative min-w-48 flex-1">
          <Search className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-muted" aria-hidden="true" />
          <input type="search" className="input pl-9" placeholder="Поиск по логу" aria-label="Поиск по логу" value={query} onChange={(e) => setQuery(e.target.value)} />
        </div>
        <Toggle id="log-only" checked={onlyMatches} onChange={setOnlyMatches} label="Только совпадения" />
        <Toggle id="log-follow" checked={follow} onChange={setFollow} label={<span className="inline-flex items-center gap-1"><ArrowDownToLine className="size-3.5" aria-hidden="true" />К концу</span>} />
        {needle ? (
          <span className="text-xs text-muted" aria-live="polite">
            совпадений: {matches}
          </span>
        ) : null}
      </div>
      <ErrorBox error={error} onRetry={reload} />
      <div
        ref={boxRef}
        className="h-[65vh] overflow-auto rounded-xl border border-border bg-bg p-3 font-mono text-xs leading-5"
        tabIndex={0}
        role="log"
        aria-label={`Содержимое ${name}`}
      >
        {loading && !data ? (
          <Spinner label="Загрузка…" />
        ) : shown.length ? (
          shown.map((line, i) => (
            <div key={i} className={`break-all whitespace-pre-wrap ${levelClass(line)}`}>
              {highlight(line, query.trim())}
            </div>
          ))
        ) : (
          <p className="text-muted">{all.length ? 'Нет совпадений' : 'Лог пуст'}</p>
        )}
      </div>
    </div>
  )
}

export function LogsPage() {
  const [params, setParams] = useSearchParams()
  const selected = params.get('name')
  const logs = usePolling(() => getLogs(), 30_000)
  const [filter, setFilter] = useState('')
  const list = useMemo(() => {
    const arr = Array.isArray(logs.data) ? logs.data : Array.isArray(logs.data?.logs) ? logs.data.logs : []
    const q = filter.trim().toLowerCase()
    return [...arr].filter((l) => !q || l.name.toLowerCase().includes(q)).sort((a, b) => a.name.localeCompare(b.name))
  }, [logs.data, filter])

  return (
    <>
      <PageHeader title="Логи" description="Файлы Data/log/*.log" />
      <ErrorBox error={logs.error} onRetry={logs.reload} />
      <FdbJournalCard onChanged={logs.reload} />
      <div className="grid gap-6 lg:grid-cols-[18rem_1fr]">
        <aside className="card min-w-0 p-3" aria-label="Файлы логов">
          <input
            type="search"
            className="input mb-2"
            placeholder="Фильтр"
            aria-label="Фильтр файлов"
            value={filter}
            onChange={(e) => setFilter(e.target.value)}
          />
          {logs.loading && !logs.data ? (
            <Spinner label="Загрузка…" />
          ) : list.length ? (
            <ul className="max-h-[70vh] space-y-0.5 overflow-y-auto">
              {list.map((l) => (
                <li key={l.name}>
                  <button
                    type="button"
                    onClick={() => setParams({ name: l.name })}
                    aria-current={selected === l.name ? 'true' : undefined}
                    className={`flex w-full items-start gap-2 rounded-lg px-2 py-1.5 text-left text-sm ${
                      selected === l.name ? 'bg-brand/15 text-accent' : 'hover:bg-surface-2'
                    }`}
                  >
                    <FileText className="mt-0.5 size-4 shrink-0" aria-hidden="true" />
                    <span className="min-w-0">
                      <span className="block truncate font-mono text-xs">{l.name}</span>
                      <span className="block text-[11px] text-muted">
                        {formatBytes(l.size)} · {formatDate(l.modified)}
                      </span>
                    </span>
                  </button>
                </li>
              ))}
            </ul>
          ) : (
            <p className="p-2 text-sm text-muted">Логов нет</p>
          )}
        </aside>
        <section className="card min-w-0 p-4">
          {selected ? <LogViewer key={selected} name={selected} /> : <p className="text-sm text-muted">Выберите файл слева.</p>}
        </section>
      </div>
    </>
  )
}
