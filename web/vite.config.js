import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig, loadEnv } from 'vite'

// Paths served by the CrabIndex backend; everything else is the SPA.
const BACKEND_PATHS = [
  '/api',
  '/stats/torrents',
  '/stats/meta',
  '/stats/tracks',
  '/health',
  '/version',
  '/lastupdatedb',
  '/docs',
  '/swagger',
  '/openapi.yaml',
]

export default defineConfig(({ mode }) => {
  const env = loadEnv(mode, process.cwd(), '')
  const target = env.VITE_API_PROXY_TARGET || 'http://127.0.0.1:9117'
  const proxy = Object.fromEntries(BACKEND_PATHS.map((p) => [p, { target, changeOrigin: true }]))

  return {
    plugins: [react(), tailwindcss()],
    server: { proxy },
    preview: { proxy },
    build: {
      outDir: 'dist',
      emptyOutDir: true,
      sourcemap: false,
    },
    test: {
      environment: 'jsdom',
      globals: true,
      setupFiles: ['./src/test/setup.js'],
      css: false,
    },
  }
})
