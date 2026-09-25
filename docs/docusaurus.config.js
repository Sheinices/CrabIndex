import { themes as prismThemes } from 'prism-react-renderer'
import httpMethodBadges from './src/remark/httpMethodBadges.js'

const GITHUB = 'https://github.com/sheinices/crabindex'
const TELEGRAM = 'https://t.me/prisma_party'

const config = {
  title: 'CrabIndex',
  tagline: 'Агрегатор торрент-трекеров для Prisma, Lampa, Sonarr и Prowlarr',
  favicon: 'img/favicon.ico',

  // The site is served by the crabindex server itself at http://<host>:9117/docs/
  url: 'http://localhost:9117',
  baseUrl: '/docs/',
  trailingSlash: true,

  onBrokenLinks: 'throw',
  markdown: {
    // .md → CommonMark (no JSX surprises), .mdx → MDX with React components
    format: 'detect',
    mermaid: true,
    hooks: { onBrokenMarkdownLinks: 'throw' },
  },

  i18n: {
    defaultLocale: 'ru',
    locales: ['ru'],
  },

  future: { v4: true, faster: true },

  presets: [
    [
      'classic',
      {
        docs: {
          routeBasePath: '/',
          sidebarPath: './sidebars.js',
          editUrl: `${GITHUB}/edit/main/docs/`,
          showLastUpdateTime: false,
          remarkPlugins: [httpMethodBadges],
        },
        blog: false,
        theme: { customCss: './src/css/custom.css' },
      },
    ],
  ],

  themes: [
    '@docusaurus/theme-mermaid',
    [
      '@easyops-cn/docusaurus-search-local',
      {
        hashed: true,
        language: ['ru', 'en'],
        docsRouteBasePath: '/',
        indexBlog: false,
        highlightSearchTermsOnTargetPage: true,
        searchResultLimits: 10,
      },
    ],
  ],

  themeConfig:
    {
      image: 'img/social-preview.png',
      colorMode: { respectPrefersColorScheme: true },
      docs: { sidebar: { hideable: true, autoCollapseCategories: true } },
      navbar: {
        title: 'CrabIndex',
        logo: { alt: 'CrabIndex', src: 'img/icon.png' },
        hideOnScroll: true,
        items: [
          { type: 'docSidebar', sidebarId: 'guide', position: 'left', label: 'Руководство' },
          { type: 'docSidebar', sidebarId: 'trackers', position: 'left', label: 'Трекеры' },
          { type: 'docSidebar', sidebarId: 'api', position: 'left', label: 'API' },
          // Links to the crabindex server itself (outside /docs/): raw HTML so baseUrl is not prepended.
          { type: 'html', position: 'right', value: '<a class="navbar__item navbar__link" href="/">Веб-интерфейс</a>' },
          { type: 'html', position: 'right', value: '<a class="navbar__item navbar__link" href="/swagger/">Swagger</a>' },
          { href: GITHUB, position: 'right', className: 'header-github-link', 'aria-label': 'GitHub' },
        ],
      },
      footer: {
        style: 'dark',
        links: [
          {
            title: 'Документация',
            items: [
              { label: 'Быстрый старт', to: '/quickstart/' },
              { label: 'Конфигурация', to: '/configuration/overview/' },
              { label: 'Справочник API', to: '/api-reference/overview/' },
            ],
          },
          {
            title: 'Сервер',
            items: [
              { html: '<a class="footer__link-item" href="/">Веб-интерфейс</a>' },
              { html: '<a class="footer__link-item" href="/swagger/">Swagger UI</a>' },
              { html: '<a class="footer__link-item" href="/openapi.yaml">openapi.yaml</a>' },
            ],
          },
          {
            title: 'Сообщество',
            items: [
              { label: 'GitHub', href: GITHUB },
              { label: 'Telegram', href: TELEGRAM },
            ],
          },
        ],
        copyright: `CrabIndex · MIT License · ${new Date().getFullYear()}`,
      },
      prism: {
        theme: prismThemes.github,
        darkTheme: prismThemes.dracula,
        additionalLanguages: ['bash', 'yaml', 'json', 'ini', 'nginx', 'docker', 'rust', 'toml'],
      },
      mermaid: { theme: { light: 'neutral', dark: 'dark' } },
    },
}

export default config
