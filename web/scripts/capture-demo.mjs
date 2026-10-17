// Capture a read-only snapshot of a running Lyceum server for the static demo.
// Usage: node scripts/capture-demo.mjs [http://127.0.0.1:8080]
import { writeFileSync } from 'node:fs'

const BASE = (process.argv[2] ?? 'http://127.0.0.1:8080') + '/api'
const PASSWORD = process.env.DEMO_PASSWORD ?? 'Northfield#2026'
const PERSONAS = ['admin', 'headmaster', 'meera', 'grace', 'priya', 'aarav']

async function session(handle) {
  const res = await fetch(`${BASE}/auth/login`, {
    method: 'POST', headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ email: `${handle}@northfield.school`, password: PASSWORD }),
  })
  if (!res.ok) throw new Error(`login ${handle}: ${res.status}`)
  const cookie = res.headers.get('set-cookie').split(';')[0]
  const out = {}
  const get = async (path) => {
    if (path in out) return out[path]
    const r = await fetch(BASE + path, { headers: { cookie } })
    if (!r.ok) return (out[path] = { __status: r.status })
    return (out[path] = await r.json())
  }
  return { get, out }
}

const arr = (v) => (Array.isArray(v) ? v : [])
const snapshot = { captured_at: new Date().toISOString(), personas: {} }
for (const h of PERSONAS) {
  const { get, out } = await session(h)
  const me = await get('/auth/me')
  const perms = new Set(me.permissions)
  const tree = await get('/org/tree')
  for (const p of ['/dashboard', '/approvals/inbox', '/approvals/mine', '/content', '/content/mine', '/feed', '/forms', '/forms/submissions/mine', '/people', '/notifications', '/auth/sessions', '/settings/public']) await get(p)
  for (const k of ['announcement', 'circular', 'page']) await get(`/content?kind=${k}`)
  for (const u of tree) await get(`/feed?unit=${u.id}`)
  const reqIds = new Set()
  const inbox = await get('/approvals/inbox')
  inbox.pending?.forEach((t) => reqIds.add(t.request_id)); inbox.recent?.forEach((t) => reqIds.add(t.request_id))
  arr(await get('/approvals/mine')).forEach((r) => reqIds.add(r.id))
  if (perms.has('approvals.view_all')) {
    for (const s of ['', 'in_progress', 'returned', 'approved', 'rejected']) {
      const rows = await get(`/approvals/all${s ? `?status=${s}` : ''}`)
      arr(rows).forEach((r) => reqIds.add(r.id))
    }
  }
  for (const id of reqIds) await get(`/approvals/${id}`)
  const content = [...arr(await get('/content')), ...arr(await get('/content/mine'))]
  for (const c of content) if (c.id) await get(`/content/${c.id}`)
  const forms = await get('/forms')
  for (const f of arr(forms)) if (f.key) await get(`/forms/${f.key}`)
  const feed = await get('/feed')
  for (const p of arr(feed)) if (p.id) await get(`/feed/${p.id}/comments`)
  if ([...perms].some((p) => p.startsWith('admin.'))) {
    const roles = await get('/admin/roles')
    await get('/admin/settings'); await get('/admin/audit'); await get('/admin/audit/verify'); await get('/admin/sessions'); await get('/admin/forms')
    const users = await get('/admin/users')
    for (const r of (roles.roles ?? [])) await get(`/admin/users?role=${r.key}`)
    for (const s of ['active', 'disabled']) await get(`/admin/users?status=${s}`)
    for (const u of tree) await get(`/admin/users?unit=${u.id}`)
    for (const u of arr(users)) if (u.id) await get(`/admin/users/${u.id}`)
    const wfs = await get('/workflows')
    for (const w of arr(wfs)) for (const v of (w.versions ?? [])) await get(`/workflows/${v.id}`)
    for (const a of ['auth.', 'user.', 'membership.', 'role.', 'workflow.', 'approval.', 'settings.', 'content.', 'post.']) await get(`/admin/audit?action=${a}`)
  }
  snapshot.personas[`${h}@northfield.school`] = out
  console.log(h, Object.keys(out).length, 'responses')
}
const json = JSON.stringify(snapshot)
writeFileSync(new URL('../src/demo/snapshot.json', import.meta.url), json)
console.log('wrote', (json.length / 1024).toFixed(0), 'KB')
