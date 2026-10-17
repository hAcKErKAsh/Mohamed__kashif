import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import '@fontsource-variable/newsreader'
import '@fontsource/ibm-plex-sans/400.css'
import '@fontsource/ibm-plex-sans/500.css'
import '@fontsource/ibm-plex-sans/600.css'
import '@fontsource/ibm-plex-mono/400.css'
import './styles.css'
import { App } from './App'
import { ToastProvider } from './components/ui'

const client = new QueryClient({
  defaultOptions: { queries: { refetchOnWindowFocus: false, retry: 1, staleTime: 15_000 } },
})

async function boot() {
  if (import.meta.env.MODE === 'demo') {
    const { installDemo } = await import('./demo/backend')
    await installDemo()
  }
  const saved = (() => {
    try {
      return localStorage.getItem('lyceum-theme')
    } catch {
      return null
    }
  })()
  if (saved === 'light' || saved === 'dark') document.documentElement.dataset.theme = saved
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <QueryClientProvider client={client}>
        <ToastProvider>
          <App />
        </ToastProvider>
      </QueryClientProvider>
    </StrictMode>,
  )
}

boot()
