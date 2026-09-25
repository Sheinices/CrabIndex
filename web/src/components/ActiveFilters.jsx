import { X } from 'lucide-react'
import { useT } from '../context.js'
import { EMPTY_FILTERS, FACETS } from '../lib/torrents.js'
import { facetValueLabel } from './filterLabels.js'

/** Removable chips for every active filter + "reset all". */
export function ActiveFilters({ filters, onChange }) {
  const t = useT()
  const chips = []
  for (const facet of FACETS) {
    for (const value of filters[facet]) {
      chips.push({
        key: `${facet}:${value}`,
        label: facetValueLabel(t, facet, value),
        remove: () => onChange({ ...filters, [facet]: filters[facet].filter((v) => v !== value) }),
      })
    }
  }
  if (filters.period) {
    chips.push({ key: 'period', label: t(`period.${filters.period}`), remove: () => onChange({ ...filters, period: '' }) })
  }
  if (filters.text.trim()) {
    chips.push({ key: 'text', label: `«${filters.text.trim()}»`, remove: () => onChange({ ...filters, text: '' }) })
  }
  if (!chips.length) return null

  return (
    <div className="flex flex-wrap items-center gap-1.5">
      {chips.map((c) => (
        <button
          key={c.key}
          type="button"
          className="chip border-brand/50 bg-brand-soft py-1 pr-2 text-[13px]"
          onClick={c.remove}
          aria-label={t('filters.remove', { label: c.label })}
        >
          {c.label}
          <X className="size-3.5 text-muted" aria-hidden />
        </button>
      ))}
      {chips.length > 1 && (
        <button type="button" className="btn btn-ghost px-2 py-1 text-[13px]" onClick={() => onChange({ ...EMPTY_FILTERS })}>
          {t('filters.resetAll')}
        </button>
      )}
    </div>
  )
}
