import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { mockApiPlugin } from './mock/mockApi.js'

// Admin UI is served by crabindex under a configurable prefix ({admin.path}),
// so every asset URL must be relative (`base: './'`).
export default defineConfig(({ command }) => ({
  base: './',
  plugins: [react(), tailwindcss(), command === 'serve' && !process.env.VITEST ? mockApiPlugin() : null].filter(Boolean),
  build: {
    outDir: '../wwwroot/admin',
    emptyOutDir: true,
    chunkSizeWarningLimit: 900,
  },
  test: {
    environment: 'jsdom',
    globals: true,
    setupFiles: ['./src/test/setup.js'],
    css: false,
  },
}))
