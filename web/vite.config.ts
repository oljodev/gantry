/// <reference types="vitest/config" />
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

// In dev, the Vite server proxies API + WebSocket traffic to the Gantry
// control plane; in production FastAPI serves the built SPA itself.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  server: {
    proxy: {
      '/api': { target: 'http://127.0.0.1:8400', ws: true },
      '/healthz': { target: 'http://127.0.0.1:8400' },
    },
  },
  test: {
    environment: 'node',
    include: ['src/**/*.test.ts'],
  },
})
