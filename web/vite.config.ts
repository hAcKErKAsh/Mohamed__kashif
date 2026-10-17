import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// `--mode demo` builds the self-contained GitHub Pages demo (no server).
export default defineConfig(({ mode }) => ({
  plugins: [react()],
  base: mode === 'demo' ? './' : '/',
  build: { outDir: mode === 'demo' ? 'dist-demo' : 'dist', sourcemap: false, chunkSizeWarningLimit: 700 },
  server: { proxy: { '/api': 'http://127.0.0.1:8080' } },
}))
