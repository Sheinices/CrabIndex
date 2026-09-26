const trackerPages = [
  'rutracker', 'kinozal', 'rutor', 'nnmclub', 'megapeer', 'toloka', 'torrentby',
  'anibelka', 'korsars', 'ultradox',
  'mazepa', 'lostfilm', 'selezen', 'baibako', 'animelayer', 'anidub', 'aniliberty', 'rudub',
  'anistar', 'anifilm', 'leproduction', 'viruseproject',
  'bitru', 'knaben', 'subsplease',
]

const sidebars = {
  guide: [
    'index',
    {
      type: 'category',
      label: 'Начало работы',
      collapsed: false,
      items: ['quickstart', 'installation', 'authentication', 'admin'],
    },
    {
      type: 'category',
      label: 'Концепции',
      items: [
        'concepts/architecture',
        'concepts/filedb',
        'concepts/search',
        'concepts/sync',
        'concepts/tracks',
        'concepts/background-jobs',
        'concepts/web-ui',
      ],
    },
    {
      type: 'category',
      label: 'Конфигурация',
      items: [
        'configuration/overview',
        'configuration/trackers',
        'configuration/search',
        'configuration/alloha',
        'configuration/tracks',
        'configuration/flaresolverr',
        'configuration/proxy',
        'configuration/logging',
      ],
    },
    {
      type: 'category',
      label: 'Развёртывание',
      items: [
        'deployment/where-to-run',
        'deployment/docker',
        'deployment/linux',
        'deployment/windows',
        'deployment/cron',
        'deployment/reverse-proxy',
        'deployment/sync-server',
      ],
    },
    {
      type: 'category',
      label: 'Эксплуатация',
      items: ['operations/waf', 'operations/access-matrix', 'operations/maintenance', 'operations/troubleshooting'],
    },
    { type: 'doc', id: 'clients/overview', label: 'Prisma, Lampa, Sonarr' },
    {
      type: 'category',
      label: 'Разработка',
      items: ['development/building', 'development/adding-trackers', 'development/docs-workflow'],
    },
  ],
  trackers: [
    'trackers/overview',
    {
      type: 'category',
      label: 'Все трекеры',
      collapsed: false,
      items: trackerPages.map((t) => `trackers/${t}`),
    },
  ],
  api: [
    'api-reference/overview',
    'api-reference/conf',
    {
      type: 'category',
      label: 'Поиск',
      collapsed: false,
      items: ['api-reference/jackett', 'api-reference/torznab', 'api-reference/prowlarr', 'api-reference/native'],
    },
    {
      type: 'category',
      label: 'Управление',
      collapsed: false,
      items: [
        'api-reference/config',
        'api-reference/stats',
        'api-reference/sync',
        'api-reference/cron',
        'api-reference/health',
        'api-reference/dev',
      ],
    },
  ],
}

export default sidebars
