import { useState } from 'react'
import Link from '@docusaurus/Link'
import useBaseUrl from '@docusaurus/useBaseUrl'
import trackers, { kindLabels } from '../data/trackers'
import styles from './TrackerGrid.module.css'

function TrackerCard({ t, linkPrefix }) {
  const icon = useBaseUrl(`/img/trackers/${t.slug}.ico`)
  return (
    <Link className={styles.card} to={`${linkPrefix}${t.slug}/`}>
      <img className={styles.icon} src={icon} alt="" loading="lazy" />
      <span className={styles.body}>
        <strong>{t.name}</strong>
        <span className={styles.meta}>
          <span className={styles.badge} data-kind={t.kind}>{kindLabels[t.kind]}</span>
          {t.auth && <span className={styles.auth}>🔑 {t.auth}</span>}
          {t.size && <span className={styles.size}>{t.size}</span>}
        </span>
      </span>
    </Link>
  )
}

/** Filterable grid of all trackers. */
export default function TrackerGrid({ linkPrefix = '/trackers/' }) {
  const [query, setQuery] = useState('')
  const [kind, setKind] = useState('all')
  const q = query.trim().toLowerCase()
  const list = trackers.filter(
    (t) => (kind === 'all' || t.kind === kind) && (!q || t.name.toLowerCase().includes(q) || t.slug.includes(q)),
  )
  return (
    <div className={styles.wrap}>
      <div className={styles.controls}>
        <input
          className={styles.search}
          type="search"
          placeholder="Найти трекер…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        {['all', ...Object.keys(kindLabels)].map((k) => (
          <button
            key={k}
            type="button"
            className={k === kind ? styles.chipActive : styles.chip}
            onClick={() => setKind(k)}
          >
            {k === 'all' ? 'все' : kindLabels[k]}
          </button>
        ))}
      </div>
      <div className={styles.grid}>
        {list.map((t) => (
          <TrackerCard key={t.slug} t={t} linkPrefix={linkPrefix} />
        ))}
        {list.length === 0 && <p>Ничего не найдено.</p>}
      </div>
    </div>
  )
}
