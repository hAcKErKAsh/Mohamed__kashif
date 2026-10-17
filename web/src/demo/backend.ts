// In-browser stand-in for the API, used only by the GitHub Pages demo build.
// It replays responses captured from a real Lyceum server (see
// scripts/capture-demo.mjs) for six demo people. Changes are not persisted.

import { ApiError, setTransport } from '../lib/api'

type Responses = Record<string, unknown>
interface Snapshot { captured_at: string; personas: Record<string, Responses> }

const KEY = 'lyceum-demo-persona'
const READ_ONLY = 'This is a read-only demo, so nothing is saved. Run the full app (see the README) to submit, approve and change things.'

function load(): string | null {
  try { return sessionStorage.getItem(KEY) } catch { return null }
}
function store(v: string | null) {
  try { v ? sessionStorage.setItem(KEY, v) : sessionStorage.removeItem(KEY) } catch { /* private mode */ }
}

const clone = <T>(v: T): T => (v === undefined ? v : JSON.parse(JSON.stringify(v)))

function lookup(data: Responses, path: string): unknown {
  if (path in data) return data[path]
  const [base, query = ''] = path.split('?')
  const params = new URLSearchParams(query)
  const q = params.get('q')?.toLowerCase()
  params.delete('q')
  const rest = params.toString()
  const hit = data[rest ? `${base}?${rest}` : base]
  if (hit === undefined) return undefined
  if (q && Array.isArray(hit)) {
    return hit.filter((row) => JSON.stringify([row.full_name, row.email, row.title]).toLowerCase().includes(q))
  }
  return hit
}

export async function installDemo() {
  const snap = (await import('./snapshot.json')).default as unknown as Snapshot
  let current = load()
  if (current && !snap.personas[current]) current = null

  setTransport(async (method, path, body) => {
    await new Promise((r) => setTimeout(r, 90))
    if (path === '/settings/public') {
      const any = Object.values(snap.personas)[0]
      return clone(any['/settings/public'])
    }
    if (method === 'POST' && path === '/auth/login') {
      const email = String((body as { email?: string })?.email ?? '').trim().toLowerCase()
      if (!snap.personas[email]) throw new ApiError(400, 'bad_request', 'In the demo, sign in as one of the people listed below.')
      current = email
      store(email)
      return { mfa_required: false, csrf_token: 'demo' }
    }
    if (!current) throw new ApiError(401, 'unauthenticated', 'authentication required')
    const data = snap.personas[current]
    if (method === 'POST' && path === '/auth/logout') {
      current = null
      store(null)
      return { ok: true }
    }
    if (method === 'POST' && path === '/notifications/read') {
      const list = data['/notifications'] as { read: boolean }[] | undefined
      list?.forEach((n) => (n.read = true))
      const me = data['/auth/me'] as { unread_notifications: number }
      me.unread_notifications = 0
      return { ok: true }
    }
    if (method !== 'GET') throw new ApiError(403, 'demo', READ_ONLY)
    const hit = lookup(data, path)
    if (hit === undefined) throw new ApiError(404, 'not_found', 'Not part of the demo data.')
    const status = (hit as { __status?: number })?.__status
    if (status) throw new ApiError(status, 'error', status === 403 ? 'you do not have permission to do that' : 'not found')
    return clone(hit)
  })
}
