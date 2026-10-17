import { useEffect, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Plus, X } from 'lucide-react'
import { api } from '../../lib/api'
import type { FieldDef } from '../../lib/types'
import { applyBranding, useSession } from '../../lib/session'
import { Field, PageHead, Spinner, Toggle, errorText, useToast } from '../../components/ui'
import { useRoles } from './Users'

type S = {
  branding: { name: string; tagline: string; accent: string; motto?: string }
  modules: Record<string, boolean>
  security: { password_min_length: number; lockout_threshold: number; lockout_minutes: number; session_idle_minutes: number; session_absolute_hours: number; mfa_required_roles: string[] }
  content_policy: { content_workflow_key: string; post_workflow_key: string }
  profile_fields: FieldDef[]
  org_kinds: string[]
}

const ACCENTS = ['#2f5d50', '#1f4e79', '#7a2e2e', '#5b3f78', '#8a5a1f', '#2e6670', '#3d4a5c', '#6b7a2e']
const MODULES: [string, string, string][] = [
  ['approvals', 'Approvals', 'Requests, the decision inbox and workflow routing.'],
  ['forms', 'Requests & forms', 'Forms people fill in to start a request.'],
  ['content', 'Notices', 'Announcements, circulars and pages.'],
  ['social', 'Community', 'Posts, comments and reactions.'],
  ['directory', 'People directory', 'Searchable list of people in your groups.'],
]

export default function Settings() {
  const { data } = useQuery({ queryKey: ['settings'], queryFn: () => api.get<S>('/admin/settings') })
  const [tab, setTab] = useState('branding')
  if (!data) return <main className="content"><Spinner /></main>
  return (
    <main className="content">
      <PageHead eyebrow="Control room" title="Modules & policy">Switch features on and off, set the school's look, and decide how strict security should be. Every change is audited.</PageHead>
      <div className="tabs">
        {[['branding', 'Branding'], ['modules', 'Modules'], ['security', 'Security policy'], ['content', 'Publishing'], ['profile', 'Profile fields'], ['kinds', 'Unit kinds']].map(([k, l]) => (
          <button key={k} className={tab === k ? 'on' : ''} onClick={() => setTab(k)}>{l}</button>
        ))}
      </div>
      {tab === 'branding' && <Branding value={data.branding} />}
      {tab === 'modules' && <Modules value={data.modules} />}
      {tab === 'security' && <Security value={data.security} />}
      {tab === 'content' && <Publishing value={data.content_policy} />}
      {tab === 'profile' && <ProfileFields value={data.profile_fields} />}
      {tab === 'kinds' && <Kinds value={data.org_kinds} />}
    </main>
  )
}

function useSave(key: string) {
  const qc = useQueryClient()
  const toast = useToast()
  const { refresh } = useSession()
  return useMutation({
    mutationFn: (v: unknown) => api.put(`/admin/settings/${key}`, v),
    onSuccess: () => { toast('Saved.'); qc.invalidateQueries({ queryKey: ['settings'] }); refresh() },
    onError: (e) => toast(errorText(e), 'bad'),
  })
}

function Branding({ value }: { value: S['branding'] }) {
  const [v, setV] = useState(value)
  const save = useSave('branding')
  useEffect(() => { applyBranding(v.accent) }, [v.accent])
  useEffect(() => () => applyBranding(value.accent), [value.accent])
  return (
    <div className="panel" style={{ maxWidth: 720 }}>
      <div className="panel-pad form-grid">
        <Field label="School name" className="full"><input className="input" value={v.name} onChange={(e) => setV({ ...v, name: e.target.value })} /></Field>
        <Field label="Tagline"><input className="input" value={v.tagline} onChange={(e) => setV({ ...v, tagline: e.target.value })} /></Field>
        <Field label="Motto" hint="Shown under the name in the sidebar."><input className="input" value={v.motto ?? ''} onChange={(e) => setV({ ...v, motto: e.target.value })} /></Field>
        <Field label="Accent colour" className="full" hint="Previewing live. Pick a deep tone so text on it stays readable.">
          <div className="row wrap" style={{ gap: 8 }}>
            {ACCENTS.map((c) => <button key={c} type="button" aria-label={c} onClick={() => setV({ ...v, accent: c })} style={{ width: 32, height: 32, borderRadius: 5, background: c, cursor: 'pointer', border: v.accent === c ? '2px solid var(--ink)' : '2px solid transparent' }} />)}
            <input className="input mono" style={{ width: 110 }} value={v.accent} onChange={(e) => setV({ ...v, accent: e.target.value })} aria-label="Hex colour" />
          </div>
        </Field>
      </div>
      <div className="panel-foot"><button className="btn primary" onClick={() => save.mutate(v)} disabled={save.isPending}>Save branding</button></div>
    </div>
  )
}

function Modules({ value }: { value: Record<string, boolean> }) {
  const [v, setV] = useState(value)
  const save = useSave('modules')
  return (
    <div className="panel ruled" style={{ maxWidth: 720 }}>
      {MODULES.map(([k, l, d]) => (
        <div key={k} className="row" style={{ padding: '16px 20px', gap: 16 }}>
          <div style={{ flex: 1 }}><div style={{ fontWeight: 500 }}>{l}</div><div className="faint" style={{ fontSize: 13 }}>{d}</div></div>
          <Toggle on={v[k] !== false} label={l} onChange={(on) => { const next = { ...v, [k]: on }; setV(next); save.mutate(next) }} />
        </div>
      ))}
    </div>
  )
}

function Security({ value }: { value: S['security'] }) {
  const [v, setV] = useState(value)
  const save = useSave('security')
  const { data: roles } = useRoles()
  const num = (k: keyof S['security']) => (e: React.ChangeEvent<HTMLInputElement>) => setV({ ...v, [k]: Number(e.target.value) })
  return (
    <div className="panel" style={{ maxWidth: 820 }}>
      <div className="panel-pad form-grid">
        <Field label="Minimum password length" hint="NIST recommends length over complexity rules."><input className="input" type="number" min={8} max={128} value={v.password_min_length} onChange={num('password_min_length')} /></Field>
        <div />
        <Field label="Lock after failed attempts"><input className="input" type="number" min={3} max={20} value={v.lockout_threshold} onChange={num('lockout_threshold')} /></Field>
        <Field label="Lock for (minutes)"><input className="input" type="number" min={1} value={v.lockout_minutes} onChange={num('lockout_minutes')} /></Field>
        <Field label="Sign out after inactivity (minutes)"><input className="input" type="number" min={5} value={v.session_idle_minutes} onChange={num('session_idle_minutes')} /></Field>
        <Field label="Maximum session length (hours)"><input className="input" type="number" min={1} value={v.session_absolute_hours} onChange={num('session_absolute_hours')} /></Field>
        <Field label="Require two-step sign-in for" className="full" hint="People in these roles are prompted to set it up and can't turn it off.">
          <div className="row wrap" style={{ gap: 14 }}>
            {roles?.roles.map((r) => (
              <label key={r.id} className="check"><input type="checkbox" checked={v.mfa_required_roles.includes(r.key)} onChange={(e) => setV({ ...v, mfa_required_roles: e.target.checked ? [...v.mfa_required_roles, r.key] : v.mfa_required_roles.filter((x) => x !== r.key) })} /> {r.name}</label>
            ))}
          </div>
        </Field>
      </div>
      <div className="panel-foot"><button className="btn primary" onClick={() => save.mutate(v)} disabled={save.isPending}>Save policy</button></div>
    </div>
  )
}

function Publishing({ value }: { value: S['content_policy'] }) {
  const [v, setV] = useState(value)
  const save = useSave('content_policy')
  const { data: workflows } = useQuery({ queryKey: ['workflows'], queryFn: () => api.get<{ key: string; versions: { name: string }[] }[]>('/workflows') })
  const pick = (k: keyof S['content_policy'], label: string, hint: string) => (
    <Field label={label} hint={hint}>
      <select className="select" value={v[k]} onChange={(e) => setV({ ...v, [k]: e.target.value })}>
        {workflows?.map((w) => <option key={w.key} value={w.key}>{w.versions[0]?.name} ({w.key})</option>)}
      </select>
    </Field>
  )
  return (
    <div className="panel" style={{ maxWidth: 720 }}>
      <div className="panel-pad stack">
        {pick('content_workflow_key', 'Notices are approved by', 'Used when someone without “publish” rights submits a notice.')}
        {pick('post_workflow_key', 'Community posts are moderated by', 'Used when someone without “post without moderation” writes a post.')}
      </div>
      <div className="panel-foot"><button className="btn primary" onClick={() => save.mutate(v)} disabled={save.isPending}>Save</button></div>
    </div>
  )
}

function ProfileFields({ value }: { value: FieldDef[] }) {
  const [v, setV] = useState(value)
  const save = useSave('profile_fields')
  const set = (i: number, p: Partial<FieldDef>) => setV(v.map((f, j) => (j === i ? { ...f, ...p } : f)))
  return (
    <div className="panel" style={{ maxWidth: 820 }}>
      <div className="panel-head"><h3>Extra details everyone can fill in on their profile</h3><button className="btn sm" onClick={() => setV([...v, { key: `field_${v.length + 1}`, label: 'New field', type: 'text' }])}><Plus /> Add</button></div>
      <div className="ruled">
        {v.length === 0 && <div className="panel-pad faint">No custom fields.</div>}
        {v.map((f, i) => (
          <div key={i} className="panel-pad row wrap" style={{ gap: 8 }}>
            <input className="input sm" style={{ flex: 2, minWidth: 160 }} value={f.label} onChange={(e) => set(i, { label: e.target.value })} aria-label="Label" />
            <input className="input sm mono" style={{ width: 150 }} value={f.key} onChange={(e) => set(i, { key: e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, '') })} aria-label="Key" />
            <select className="select sm" style={{ width: 120 }} value={f.type} onChange={(e) => set(i, { type: e.target.value as FieldDef['type'], options: e.target.value === 'select' ? f.options ?? ['A', 'B'] : undefined })} aria-label="Type">
              {['text', 'textarea', 'number', 'date', 'select', 'email'].map((t) => <option key={t}>{t}</option>)}
            </select>
            {f.type === 'select' && <input className="input sm" style={{ flex: 2, minWidth: 160 }} value={f.options?.join(', ')} onChange={(e) => set(i, { options: e.target.value.split(',').map((s) => s.trim()).filter(Boolean) })} aria-label="Options" />}
            <button className="btn ghost icon sm" aria-label="Remove" onClick={() => setV(v.filter((_, j) => j !== i))}><X /></button>
          </div>
        ))}
      </div>
      <div className="panel-foot"><button className="btn primary" onClick={() => save.mutate(v)} disabled={save.isPending}>Save fields</button></div>
    </div>
  )
}

function Kinds({ value }: { value: string[] }) {
  const [v, setV] = useState(value)
  const [add, setAdd] = useState('')
  const save = useSave('org_kinds')
  return (
    <div className="panel" style={{ maxWidth: 720 }}>
      <div className="panel-pad stack">
        <p className="muted" style={{ margin: 0 }}>The kinds of unit your organisation tree uses, from broadest to narrowest. Workflows can find approvers “at the nearest grade”, and so on.</p>
        <div className="row wrap" style={{ gap: 8 }}>
          {v.map((k) => <span key={k} className="tag" style={{ height: 28 }}>{k}<button className="btn ghost icon sm" style={{ width: 18, height: 18 }} aria-label={`Remove ${k}`} onClick={() => setV(v.filter((x) => x !== k))}><X size={12} /></button></span>)}
        </div>
        <form className="row" onSubmit={(e) => { e.preventDefault(); if (add.trim() && !v.includes(add.trim())) { setV([...v, add.trim().toLowerCase()]); setAdd('') } }}>
          <input className="input" style={{ maxWidth: 240 }} placeholder="e.g. campus" value={add} onChange={(e) => setAdd(e.target.value)} />
          <button className="btn">Add</button>
        </form>
      </div>
      <div className="panel-foot"><button className="btn primary" onClick={() => save.mutate(v)} disabled={save.isPending}>Save</button></div>
    </div>
  )
}
