import { ArrowDown, ArrowUp, Check, Copy, ExternalLink, HardDrive, Magnet } from 'lucide-react'
import { memo, useState } from 'react'
import { useApp } from '../context.js'
import { copyText, isHttpUrl, isMagnet } from '../lib/clipboard.js'
import {
  formatDate,
  formatDateTime,
  formatSeasons,
  formatSize,
  qualityLabel,
  trackerIcon,
  trackerLabel,
} from '../lib/torrents.js'

const MAX_VOICES = 4

function QualityBadge({ quality }) {
  const label = qualityLabel(quality)
  if (!label) return null
  const q = Number(quality)
  const tone =
    q >= 2160
      ? 'bg-brand text-brand-fg'
      : q >= 1080
        ? 'bg-fg/90 text-bg'
        : 'bg-surface-3 text-fg'
  return <span className={`badge font-semibold ${tone}`}>{label}</span>
}

export const TorrentCard = memo(function TorrentCard({ item }) {
  const { t, locale, showToast } = useApp()
  const [copied, setCopied] = useState(false)
  const magnet = isMagnet(item.magnet) ? item.magnet : ''
  const page = isHttpUrl(item.url) ? item.url : ''
  const seasons = formatSeasons(item.seasons)
  const seasonCount = Array.isArray(item.seasons) ? item.seasons.length : 0
  const voices = Array.isArray(item.voices) ? item.voices.filter(Boolean) : []
  const sid = Number(item.sid) || 0
  const pir = Number(item.pir) || 0
  const size = formatSize(item, locale)
  const added = formatDate(item.createTime, locale)

  async function copy() {
    try {
      await copyText(magnet)
      setCopied(true)
      showToast(t('torrent.copied'))
      window.setTimeout(() => setCopied(false), 1600)
    } catch {
      showToast(t('torrent.copyFailed'), 'error')
    }
  }

  return (
    <article className="card p-4 transition-colors hover:border-line-strong sm:p-5">
      <div className="flex items-center gap-2 text-xs text-muted">
        <img
          src={trackerIcon(item.tracker)}
          alt=""
          width="16"
          height="16"
          loading="lazy"
          className="size-4 rounded-sm"
          onError={(e) => {
            if (!e.currentTarget.src.endsWith('/default.ico')) e.currentTarget.src = '/img/ico/default.ico'
          }}
        />
        <span className="font-medium text-fg/80">{trackerLabel(item.tracker)}</span>
        {added && (
          <>
            <span aria-hidden className="text-faint">
              ·
            </span>
            <time dateTime={String(item.createTime)} title={`${t('torrent.added')}: ${formatDateTime(item.createTime, locale)}`}>
              {added}
            </time>
          </>
        )}
      </div>

      <h3 className="mt-1.5 text-[15px] leading-snug font-medium break-words sm:text-base">
        {page ? (
          <a href={page} target="_blank" rel="noopener noreferrer" className="hover:text-brand">
            {item.title || item.name}
          </a>
        ) : (
          item.title || item.name
        )}
      </h3>

      <div className="mt-2.5 flex flex-wrap gap-1.5">
        <QualityBadge quality={item.quality} />
        {String(item.videotype).toLowerCase() === 'hdr' && (
          <span className="badge bg-amber-500/15 font-semibold text-amber-700 dark:text-amber-300">HDR</span>
        )}
        {(item.types || []).slice(0, 2).map((type) => (
          <span key={type} className="badge">
            {t(`type.${type}`)}
          </span>
        ))}
        {seasons && (
          <span className="badge bg-sky-500/12 text-sky-700 dark:text-sky-300">
            {t(seasonCount > 1 ? 'torrent.seasons' : 'torrent.season', { s: seasons })}
          </span>
        )}
        {Number(item.relased) > 0 && <span className="badge">{item.relased}</span>}
        {voices.slice(0, MAX_VOICES).map((v) => (
          <span key={v} className="badge bg-transparent ring-1 ring-line ring-inset">
            {v}
          </span>
        ))}
        {voices.length > MAX_VOICES && (
          <span className="badge bg-transparent ring-1 ring-line ring-inset" title={voices.slice(MAX_VOICES).join(', ')}>
            {t('torrent.moreVoices', { n: voices.length - MAX_VOICES })}
          </span>
        )}
      </div>

      <div className="mt-4 flex flex-wrap items-center justify-between gap-x-4 gap-y-3">
        <dl className="flex items-center gap-4 text-sm tabular-nums">
          {size && (
            <div className="flex items-center gap-1.5" title={t('torrent.size')}>
              <dt>
                <HardDrive className="size-4 text-faint" aria-hidden />
                <span className="sr-only">{t('torrent.size')}</span>
              </dt>
              <dd className="font-medium">{size}</dd>
            </div>
          )}
          <div className="flex items-center gap-1" title={t('torrent.seeders')}>
            <dt>
              <ArrowUp className={`size-4 ${sid > 0 ? 'text-ok' : 'text-faint'}`} aria-hidden />
              <span className="sr-only">{t('torrent.seeders')}</span>
            </dt>
            <dd className={sid > 0 ? 'font-semibold text-ok' : 'text-faint'}>{sid}</dd>
          </div>
          <div className="flex items-center gap-1" title={t('torrent.peers')}>
            <dt>
              <ArrowDown className="size-4 text-faint" aria-hidden />
              <span className="sr-only">{t('torrent.peers')}</span>
            </dt>
            <dd className="text-muted">{pir}</dd>
          </div>
        </dl>

        <div className="flex items-center gap-1.5">
          {magnet && (
            <>
              <button type="button" className="btn btn-outline h-9" onClick={copy} aria-label={t('torrent.copy')} title={t('torrent.copy')}>
                {copied ? <Check className="size-4 text-ok" aria-hidden /> : <Copy className="size-4" aria-hidden />}
                <span>{t('torrent.copyShort')}</span>
              </button>
              <a href={magnet} className="btn btn-outline h-9" aria-label={t('torrent.open')} title={t('torrent.open')}>
                <Magnet className="size-4" aria-hidden />
                <span className="hidden sm:inline">{t('torrent.openShort')}</span>
              </a>
            </>
          )}
          {page && (
            <a
              href={page}
              target="_blank"
              rel="noopener noreferrer"
              className="btn btn-ghost h-9"
              aria-label={t('torrent.page')}
              title={t('torrent.page')}
            >
              <ExternalLink className="size-4" aria-hidden />
              <span className="hidden sm:inline">{t('torrent.pageShort')}</span>
            </a>
          )}
        </div>
      </div>
    </article>
  )
})

export function TorrentCardSkeleton() {
  return (
    <div className="card p-4 sm:p-5" aria-hidden>
      <div className="skeleton h-3 w-32" />
      <div className="skeleton mt-3 h-4 w-11/12" />
      <div className="skeleton mt-2 h-4 w-2/3" />
      <div className="mt-3 flex gap-1.5">
        <div className="skeleton h-5 w-10" />
        <div className="skeleton h-5 w-14" />
        <div className="skeleton h-5 w-16" />
      </div>
      <div className="mt-4 flex justify-between">
        <div className="skeleton h-5 w-40" />
        <div className="skeleton h-9 w-44" />
      </div>
    </div>
  )
}
