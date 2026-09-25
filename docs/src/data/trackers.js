// Tracker catalogue shown on the landing page and in the trackers overview.
// kind: how the tracker is crawled; auth: what credentials it needs.
const trackers = [
  { slug: 'rutracker', name: 'Rutracker', kind: 'trio', auth: 'FlareSolverr', size: '1,5 млн' },
  { slug: 'kinozal', name: 'Kinozal', kind: 'trio', auth: 'логин', size: '550 тыс.' },
  { slug: 'rutor', name: 'Rutor', kind: 'trio', auth: null, size: '550 тыс.' },
  { slug: 'bitru', name: 'BitRu', kind: 'api', auth: null, size: '175 тыс.' },
  { slug: 'nnmclub', name: 'NNM-Club', kind: 'trio', auth: null, size: '145 тыс.' },
  { slug: 'megapeer', name: 'Megapeer', kind: 'trio', auth: null, size: '113 тыс.' },
  { slug: 'knaben', name: 'Knaben', kind: 'api', auth: null, size: '106 тыс.' },
  { slug: 'toloka', name: 'Toloka', kind: 'trio', auth: 'логин', size: '58 тыс.' },
  { slug: 'torrentby', name: 'Torrent.by', kind: 'trio', auth: null, size: '58 тыс.' },
  { slug: 'mazepa', name: 'Mazepa', kind: 'parse', auth: 'логин', size: '51 тыс.' },
  { slug: 'lostfilm', name: 'LostFilm', kind: 'parse', auth: 'cookie', size: '18 тыс.' },
  { slug: 'selezen', name: 'Selezen', kind: 'parse', auth: 'логин', size: '16 тыс.' },
  { slug: 'animelayer', name: 'AnimeLayer', kind: 'parse', auth: 'логин', size: '6 тыс.' },
  { slug: 'aniliberty', name: 'AniLiberty', kind: 'api', auth: null, size: '5 тыс.' },
  { slug: 'anidub', name: 'AniDub', kind: 'parse', auth: null, size: '4 тыс.' },
  { slug: 'anibelka', name: 'Anibelka', kind: 'trio', auth: null, size: null },
  { slug: 'korsars', name: 'Korsars', kind: 'trio', auth: 'логин', size: null },
  { slug: 'ultradox', name: 'Ultradox', kind: 'trio', auth: null, size: null },
  { slug: 'subsplease', name: 'SubsPlease', kind: 'api', auth: null, size: null },
  { slug: 'rudub', name: 'RuDub', kind: 'parse', auth: 'логин', size: null },
  { slug: 'baibako', name: 'BaibaKo', kind: 'parse', auth: 'логин', size: null },
  { slug: 'anistar', name: 'AniStar', kind: 'daily', auth: null, size: null },
  { slug: 'anifilm', name: 'AniFilm', kind: 'daily', auth: 'логин', size: null },
  { slug: 'leproduction', name: 'LE-Production', kind: 'daily', auth: null, size: null },
  { slug: 'viruseproject', name: 'ViruseProject', kind: 'daily', auth: null, size: null },
]

export const kindLabels = {
  trio: 'полный обход',
  api: 'API',
  parse: 'постранично',
  daily: 'раз в сутки',
}

export default trackers
