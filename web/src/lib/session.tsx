import { createContext, ReactNode, useContext, useEffect } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError, setCsrf } from './api'
import type { Me } from './types'

interface Session {
  me: Me
  can: (perm: string) => boolean
  module: (name: string) => boolean
  refresh: () => Promise<unknown>
  logout: () => Promise<void>
}

const Ctx = createContext<Session | null>(null)

export function useSession() {
  const s = useContext(Ctx)
  if (!s) throw new Error('useSession outside provider')
  return s
}

export function useMeQuery() {
  return useQuery({
    queryKey: ['me'],
    queryFn: async () => {
      try {
        const me = await api.get<Me>('/auth/me')
        setCsrf(me.csrf_token)
        return me
      } catch (e) {
        if (e instanceof ApiError && e.status === 401) return null
        throw e
      }
    },
    staleTime: 60_000,
    retry: false,
  })
}

export function applyBranding(accent?: string, name?: string) {
  if (accent && /^#[0-9a-f]{6}$/i.test(accent)) document.documentElement.style.setProperty('--brand', accent)
  if (name) document.title = name
}

export function SessionProvider({ me, children }: { me: Me; children: ReactNode }) {
  const qc = useQueryClient()
  useEffect(() => applyBranding(me.settings.branding.accent, me.settings.branding.name), [me.settings.branding])
  const perms = new Set(me.permissions)
  const value: Session = {
    me,
    can: (p) => perms.has(p),
    module: (m) => me.settings.modules[m] !== false,
    refresh: () => qc.invalidateQueries({ queryKey: ['me'] }),
    logout: async () => {
      try {
        await api.post('/auth/logout')
      } finally {
        qc.clear()
        qc.setQueryData(['me'], null)
      }
    },
  }
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}
