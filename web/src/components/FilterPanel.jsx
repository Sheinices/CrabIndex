import { useId, useState } from 'react'
import { useT } from '../context.js'
import { FACETS, PERIODS, trackerIcon } from '../lib/torrents.js'
import { facetValueLabel } from './filterLabels.js'

const COLLAPSED = { voice: 10, tracker: 8, year: 8, season: 12 }

function FacetGroup({ facet, options, selected, onToggle }) {
  const t = useT()
  const [expanded, setExpanded] = useState(false)
  const headingId = useId()
  const limit = COLLAPSED[facet] || 12
  // Keep selected values visible even when the list is collapsed.
  const visible = expanded ? options : options.filter((o, i) => i < limit || selected.includes(o.value))
  const hidden = options.length - visible.length

  return (
    <section aria-labelledby={headingId}>
      <h3 id={headingId} className="mb-2 text-xs font-semibold tracking-wide text-faint uppercase">
        {t(`facet.${facet}`)}
      </h3>
      <div className="flex flex-wrap gap-1.5">
        {visible.map(({ value, count }) => {
          const on = selected.includes(value)
          return (
            <button
              key={value}
              type="button"
              className="chip py-1 text-[13px]"
              aria-pressed={on}
              disabled={!on && count === 0}
              onClick={() => onToggle(facet, value)}
            >
              {facet === 'tracker' && <img src={trackerIcon(value)} alt="" width="14" height="14" className="size-3.5 rounded-sm" />}
              <span>{facet === 'season' ? value : facetValueLabel(t, facet, value)}</span>
              <span className="text-xs text-faint tabular-nums">{count}</span>
            </button>
          )
        })}
        {(hidden > 0 || expanded) && options.length > limit && (
          <button type="button" className="chip border-dashed py-1 text-[13px] text-muted" onClick={() => setExpanded((v) => !v)}>
            {expanded ? t('filters.less') : t('filters.more', { n: hidden })}
          </button>
        )}
      </div>
    </section>
  )
}

/** Every filter control. Rendered in the desktop sidebar and in the mobile sheet. */
export function FilterPanel({ facets, filters, onChange }) {
  const t = useT()
  const textId = useId()

  function toggle(facet, value) {
    const current = filters[facet]
    const next = current.includes(value) ? current.filter((v) => v !== value) : [...current, value]
    onChange({ ...filters, [facet]: next })
  }

  return (
    <div className="space-y-6">
      <div>
        <label htmlFor={textId} className="mb-2 block text-xs font-semibold tracking-wide text-faint uppercase">
          {t('filters.within')}
        </label>
        <input
          id={textId}
          type="text"
          className="input"
          value={filters.text}
          placeholder={t('filters.withinPlaceholder')}
          onChange={(e) => onChange({ ...filters, text: e.target.value })}
          autoComplete="off"
          spellCheck={false}
        />
      </div>

      {FACETS.map((facet) => {
        const options = facets[facet] || []
        // A facet with a single value filters nothing - hide it unless it is active.
        if (options.length < 2 && !filters[facet].length) return null
        return <FacetGroup key={facet} facet={facet} options={options} selected={filters[facet]} onToggle={toggle} />
      })}

      <fieldset>
        <legend className="mb-2 text-xs font-semibold tracking-wide text-faint uppercase">{t('facet.period')}</legend>
        <div className="flex flex-wrap gap-1.5">
          {['', ...PERIODS].map((p) => (
            <button
              key={p || 'any'}
              type="button"
              className="chip py-1 text-[13px]"
              aria-pressed={filters.period === p}
              onClick={() => onChange({ ...filters, period: p })}
            >
              {t(`period.${p || 'any'}`)}
            </button>
          ))}
        </div>
      </fieldset>
    </div>
  )
}
