import { ArrowDownWideNarrow, ArrowUpNarrowWide, CircleAlert, History, KeyRound, RotateCcw, SearchX, SlidersHorizontal, X } from 'lucide-react'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import { ActiveFilters } from '../components/ActiveFilters.jsx'
import { ApiKeyForm } from '../components/ApiKeyForm.jsx'
import { FilterPanel } from '../components/FilterPanel.jsx'
import { FilterSheet } from '../components/FilterSheet.jsx'
import { SearchForm } from '../components/SearchForm.jsx'
import { TorrentCard, TorrentCardSkeleton } from '../components/TorrentCard.jsx'
import { useApp } from '../context.js'
import { useTorrentSearch } from '../hooks/useTorrentSearch.js'
import { pluralTorrents } from '../lib/i18n.js'
import { clearRecentSearches, getRecentSearches, pushRecentSearch, removeRecentSearch } from '../lib/recent.js'
import {
  buildFacets,
  countActiveFilters,
  EMPTY_FILTERS,
  filterItems,
  filtersFromParams,
  SORT_KEYS,
  sortFromParams,
  sortItems,
  stateToParams,
  torrentKey,
} from '../lib/torrents.js'

const PAGE_SIZE = 40
const EXAMPLES = ['Динозавры', 'Интерстеллар', 'tt0133093', 'kp301']

function StateBlock({ icon: Icon, title, children, tone = 'muted' }) {
  return (
    <div className="card flex flex-col items-center px-6 py-14 text-center">
      <div className={`mb-4 flex size-12 items-center justify-center rounded-full ${tone === 'error' ? 'bg-red-500/10 text-red-500' : 'bg-surface-2 text-muted'}`}>
        <Icon className="size-6" aria-hidden />
      </div>
      <h2 className="text-lg font-semibold">{title}</h2>
      <div className="mt-2 max-w-md text-sm text-muted">{children}</div>
    </div>
  )
}

function SortControl({ sort, onChange }) {
  const { t } = useApp()
  const DirIcon = sort.dir === 'asc' ? ArrowUpNarrowWide : ArrowDownWideNarrow
  return (
    <div className="flex items-center gap-1">
      <label htmlFor="sort" className="sr-only">
        {t('sort.label')}
      </label>
      <select
        id="sort"
        value={sort.key}
        onChange={(e) => onChange({ ...sort, key: e.target.value })}
        className="h-9 rounded-lg border border-line bg-surface py-0 pr-8 pl-3 text-sm text-fg focus:border-brand focus:outline-none"
      >
        {SORT_KEYS.map((k) => (
          <option key={k} value={k}>
            {t(`sort.${k}`)}
          </option>
        ))}
      </select>
      <button
        type="button"
        className="icon-btn border border-line bg-surface"
        onClick={() => onChange({ ...sort, dir: sort.dir === 'asc' ? 'desc' : 'asc' })}
        aria-label={sort.dir === 'asc' ? t('sort.asc') : t('sort.desc')}
        title={sort.dir === 'asc' ? t('sort.asc') : t('sort.desc')}
      >
        <DirIcon className="size-4" />
      </button>
    </div>
  )
}

function Hero({ onSearch, recent, onRemoveRecent, onClearRecent }) {
  const { t } = useApp()
  return (
    <div className="mx-auto max-w-2xl pt-10 sm:pt-20">
      <div className="mb-8 text-center">
        <img src="/img/icon-192.png" alt="" width="96" height="96" className="mx-auto size-20 sm:size-24" />
        <p className="mb-6 mt-2 text-3xl font-extrabold tracking-tight sm:text-4xl" aria-label="CrabIndex">
          <span className="text-brand">Crab</span>
          <span>Index</span>
        </p>
        <h1 className="text-2xl font-semibold tracking-tight sm:text-3xl">{t('search.heroTitle')}</h1>
        <p className="mx-auto mt-3 max-w-lg text-sm text-muted sm:text-base">{t('search.heroText')}</p>
      </div>
      <SearchForm query="" onSearch={onSearch} large />
      <div className="mt-6 space-y-5">
        {recent.length > 0 && (
          <section aria-labelledby="recent-title">
            <div className="mb-2 flex items-center justify-between">
              <h2 id="recent-title" className="flex items-center gap-1.5 text-xs font-semibold tracking-wide text-faint uppercase">
                <History className="size-3.5" aria-hidden />
                {t('search.recent')}
              </h2>
              <button type="button" className="text-xs text-muted hover:text-fg" onClick={onClearRecent}>
                {t('search.recentClear')}
              </button>
            </div>
            <ul className="flex flex-wrap gap-1.5">
              {recent.map((q) => (
                <li key={q} className="chip gap-0 p-0">
                  <button type="button" className="rounded-l-full py-1.5 pr-1.5 pl-3" onClick={() => onSearch(q)}>
                    {q}
                  </button>
                  <button
                    type="button"
                    className="rounded-r-full py-1.5 pr-2.5 pl-1 text-faint hover:text-fg"
                    onClick={() => onRemoveRecent(q)}
                    aria-label={t('search.recentRemove', { q })}
                  >
                    <X className="size-3.5" aria-hidden />
                  </button>
                </li>
              ))}
            </ul>
          </section>
        )}
        <p className="flex flex-wrap items-center justify-center gap-1.5 text-sm text-muted">
          <span>{t('search.examples')}:</span>
          {EXAMPLES.map((ex) => (
            <button key={ex} type="button" className="rounded-md px-1.5 py-0.5 font-medium text-fg/80 underline decoration-line-strong underline-offset-4 hover:text-brand" onClick={() => onSearch(ex)}>
              {ex}
            </button>
          ))}
        </p>
      </div>
    </div>
  )
}

export function SearchPage() {
  const { t, locale, apiKey, conf } = useApp()
  const [params, setParams] = useSearchParams()
  const q = (params.get('q') || '').trim()
  const filters = useMemo(() => filtersFromParams(params), [params])
  const sort = useMemo(() => sortFromParams(params), [params])
  const [recent, setRecent] = useState(getRecentSearches)
  const [sheetOpen, setSheetOpen] = useState(false)
  const [page, setPage] = useState({ sig: '', count: PAGE_SIZE })

  const onSuccess = useCallback((query, items) => {
    if (items.length) setRecent(pushRecentSearch(query))
  }, [])
  const search = useTorrentSearch(q, apiKey, { onSuccess })

  useEffect(() => {
    document.title = q ? `${q} - CrabIndex` : 'CrabIndex'
  }, [q])

  const now = search.fetchedAt
  const facets = useMemo(() => buildFacets(search.items, filters, { now }), [search.items, filters, now])
  const results = useMemo(() => sortItems(filterItems(search.items, filters, { now }), sort.key, sort.dir), [search.items, filters, sort, now])
  const activeCount = countActiveFilters(filters)

  const sig = `${q}|${params.toString()}|${search.items.length}`
  const shownCount = page.sig === sig ? page.count : PAGE_SIZE
  const shown = results.slice(0, shownCount)

  const runSearch = (next) => {
    if (!next) {
      setParams(new URLSearchParams())
      return
    }
    setParams(stateToParams({ q: next, filters: EMPTY_FILTERS, sort }))
    if (next === q) search.retry()
  }
  const setFilters = (next) => setParams(stateToParams({ q, filters: next, sort }), { replace: true })
  const setSort = (next) => setParams(stateToParams({ q, filters, sort: next }), { replace: true })

  const needsKey =
    (search.status === 'error' && search.error?.unauthorized) || (conf.configured && !conf.loading && !conf.valid && !apiKey)

  if (!q) {
    return (
      <div className="px-4 pb-16 sm:px-6">
        <Hero
          onSearch={runSearch}
          recent={recent}
          onRemoveRecent={(x) => setRecent(removeRecentSearch(x))}
          onClearRecent={() => setRecent(clearRecentSearches())}
        />
        {conf.configured && !conf.loading && !conf.valid && (
          <div className="card mx-auto mt-8 max-w-md p-5">
            <p className="mb-3 flex items-center gap-2 text-sm font-medium">
              <KeyRound className="size-4 text-brand" aria-hidden />
              {t('apikey.required')}
            </p>
            <ApiKeyForm compact />
          </div>
        )}
      </div>
    )
  }

  const total = search.items.length
  let body
  if (needsKey) {
    body = (
      <StateBlock icon={KeyRound} title={t('apikey.required')}>
        <div className="mt-3 w-full max-w-sm text-left">
          <ApiKeyForm compact autoFocus />
        </div>
      </StateBlock>
    )
  } else if (search.status === 'loading') {
    body = (
      <div className="space-y-3" aria-busy="true" aria-label={t('search.loading')}>
        {Array.from({ length: 5 }, (_, i) => (
          <TorrentCardSkeleton key={i} />
        ))}
      </div>
    )
  } else if (search.status === 'error') {
    const status = search.error?.status
    body = (
      <StateBlock icon={CircleAlert} title={t('search.errorTitle')} tone="error">
        <p>{status ? t('search.errorHttp', { status }) : t('search.errorNetwork')}</p>
        <button type="button" className="btn btn-outline mt-4" onClick={search.retry}>
          <RotateCcw className="size-4" aria-hidden />
          {t('search.retry')}
        </button>
      </StateBlock>
    )
  } else if (q.length < 2) {
    body = <StateBlock icon={SearchX} title={t('search.tooShort')} />
  } else if (total === 0) {
    body = (
      <StateBlock icon={SearchX} title={t('search.emptyTitle')}>
        {t('search.emptyText', { q })}
      </StateBlock>
    )
  } else if (results.length === 0) {
    body = (
      <StateBlock icon={SlidersHorizontal} title={t('search.filteredTitle')}>
        <p>{t('search.filteredText', { n: total })}</p>
        <button type="button" className="btn btn-outline mt-4" onClick={() => setFilters(EMPTY_FILTERS)}>
          {t('filters.resetAll')}
        </button>
      </StateBlock>
    )
  } else {
    body = (
      <>
        <ul className="space-y-3">
          {shown.map((item, i) => (
            <li key={torrentKey(item, i)}>
              <TorrentCard item={item} />
            </li>
          ))}
        </ul>
        <div className="mt-6 flex flex-col items-center gap-3 text-sm text-muted">
          {results.length > shown.length && (
            <>
              <span>{t('search.shown', { shown: shown.length, total: results.length })}</span>
              <button type="button" className="btn btn-outline" onClick={() => setPage({ sig, count: shownCount + PAGE_SIZE })}>
                {t('search.showMore')}
              </button>
            </>
          )}
        </div>
      </>
    )
  }

  const hasResults = search.status === 'done' && total > 0 && !needsKey
  const panel = <FilterPanel facets={facets} filters={filters} onChange={setFilters} />

  return (
    <div className="mx-auto max-w-7xl px-4 pb-16 sm:px-6">
      <div className="pt-5 sm:pt-6">
        <SearchForm query={q} onSearch={runSearch} loading={search.status === 'loading'} />
      </div>

      <div className="mt-5 lg:grid lg:grid-cols-[17rem_minmax(0,1fr)] lg:gap-8">
        {hasResults && (
          <aside className="hidden lg:block" aria-label={t('filters.title')}>
            <div className="sticky top-20 max-h-[calc(100dvh-6rem)] overflow-y-auto pr-1 pb-4 [scrollbar-width:thin]">
              <div className="mb-4 flex items-center justify-between">
                <h2 className="text-sm font-semibold">{t('filters.title')}</h2>
                {activeCount > 0 && (
                  <button type="button" className="text-xs text-muted hover:text-fg" onClick={() => setFilters(EMPTY_FILTERS)}>
                    {t('filters.reset')}
                  </button>
                )}
              </div>
              {panel}
            </div>
          </aside>
        )}

        <section aria-live="polite" aria-busy={search.status === 'loading'} className={hasResults ? '' : 'lg:col-span-2'}>
          {hasResults && (
            <div className="sticky top-14 z-20 -mx-4 mb-3 border-b border-line bg-bg/90 px-4 py-2.5 backdrop-blur-md sm:-mx-6 sm:px-6 lg:mx-0 lg:rounded-b-xl lg:px-0">
              <div className="flex flex-wrap items-center gap-2">
                <p className="mr-auto text-sm">
                  <span className="font-semibold tabular-nums">{results.length}</span>{' '}
                  <span className="text-muted">
                    {pluralTorrents(results.length, locale)}
                    {results.length !== total && ` / ${total}`}
                  </span>
                </p>
                <button type="button" className="btn btn-outline h-9 lg:hidden" onClick={() => setSheetOpen(true)}>
                  <SlidersHorizontal className="size-4" aria-hidden />
                  {t('filters.open')}
                  {activeCount > 0 && (
                    <span className="rounded-full bg-brand px-1.5 text-xs leading-5 font-semibold text-brand-fg tabular-nums">{activeCount}</span>
                  )}
                </button>
                <SortControl sort={sort} onChange={setSort} />
              </div>
              {activeCount > 0 && (
                <div className="mt-2.5">
                  <ActiveFilters filters={filters} onChange={setFilters} />
                </div>
              )}
            </div>
          )}
          {body}
        </section>
      </div>

      <FilterSheet open={sheetOpen && hasResults} onClose={() => setSheetOpen(false)} onReset={() => setFilters(EMPTY_FILTERS)} resultCount={results.length}>
        {panel}
      </FilterSheet>
    </div>
  )
}
