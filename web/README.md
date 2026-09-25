# CrabIndex public web UI

Search and statistics pages served by `crabindex` from `wwwroot/`.
React 19 + JavaScript (JSX), Vite, react-router, Tailwind CSS v4, lucide-react.

```bash
npm install
npm run dev     # http://localhost:5173, proxies the API to http://127.0.0.1:9117
npm test        # Vitest + React Testing Library
npm run lint
npm run build   # → dist/ (copied into wwwroot/ by scripts/build-web-ui.sh)
```

Set `VITE_API_PROXY_TARGET` to proxy to another server during development.

- `src/lib/` - API client, filters/sort/facets, stats helpers, i18n dictionary (ru/en).
- `src/pages/` - `/` search, `/stats` statistics.
- `public/sw.js` - kill-switch for the service worker registered by older releases; do not remove.
- `public/theme-init.js` - applies the saved theme before first paint (the server CSP forbids inline scripts).
