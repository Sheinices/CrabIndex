import { qualityLabel, trackerLabel } from '../lib/torrents.js'

/** Human label for a facet value. */
export function facetValueLabel(t, facet, value) {
  switch (facet) {
    case 'type':
      return t(`type.${value}`)
    case 'quality':
      return qualityLabel(value)
    case 'video':
      return t(`video.${value}`)
    case 'tracker':
      return trackerLabel(value)
    case 'season':
      return t('torrent.season', { s: value })
    default:
      return value
  }
}
