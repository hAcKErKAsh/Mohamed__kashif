import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowLeft, Copy, KeyRound, Lock, LogOut, Plus, Search, Trash2, UserPlus } from 'lucide-react'
import { api, qs } from '../../lib/api'
import type { Role } from '../../lib/types'
import { useSession } from '../../lib/session'
import { unitLabel, unitsWithPerm, useUnits } from '../../lib/units'
import { Avatar, Empty, Field, Modal, Notice, PageHead, Spinner, StatusTag, ago, errorText, fmtDate, useToast } from '../../components/ui'

interface Membership { membership_id: string; role_key: string; role_name: string; role_color: string; unit_id: string; unit_name: string }
interface UserRow { id: string; email: string; full_name: string; status: string; last_login_at: string | null; locked: boolean; totp_enabled: boolean; memberships: Membership[] }

export function useRoles() {
  return useQuery({ queryKey: ['roles'], queryFn: () => api.get<{ roles: Role[]; catalog: { key: string; group: string; label: string }[] }>('/admin/roles') })
}

export default function Users() {
  const [q, setQ] = useState('')
  const [role, setRole] = useState('')
  const [status, setStatus] = useState('')
  const [creating, setCreating] = useState(false)
  const nav = useNavigate()
  const { data: roles } = useRoles()
  const { data } = useQuery({ queryKey: ['admin-users', q, role, status], queryFn: () => api.get<UserRow[]>(`/admin/users${qs({ q, role, status })}`), placeholderData: (p) => p })
  return (
    <main className="content wide" style={{ maxWidth: 1280 }}>
      <PageHead eyebrow="Control room" title="People & access" actions={<button className="btn primary" onClick={() => setCreating(true)}><UserPlus /> Add person</button>}>
        Accounts and the roles they hold. A role applies at one place in the organisation and everywhere beneath it.
      </PageHead>
      <div className="row wrap" style={{ marginBottom: 14 }}>
        <div className="search" style={{ width: 300 }}><Search /><input className="input" placeholder="Name or email" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search" /></div>
        <select className="select" style={{ width: 'auto' }} value={role} onChange={(e) => setRole(e.target.value)} aria-label="Role">
          <option value="">Every role</option>
          {roles?.roles.map((r) => <option key={r.id} value={r.key}>{r.name}</option>)}
        </select>
        <select className="select" style={{ width: 'auto' }} value={status} onChange={(e) => setStatus(e.target.value)} aria-label="Status">
          <option value="">Any status</option><option value="active">Active</option><option value="disabled">Disabled</option>
        </select>
        <span className="faint" style={{ fontSize: 13, marginLeft: 'auto' }}>{data?.length ?? 0} people</span>
      </div>
      <div className="panel table-wrap">
        {!data ? <Spinner /> : data.length === 0 ? <Empty title="No one matches" /> : (
          <table className="table">
            <thead><tr><th>Person</th><th>Roles</th><th>Security</th><th>Last sign-in</th><th>Status</th></tr></thead>
            <tbody>
              {data.map((u) => (
                <tr key={u.id} className="clickable" onClick={() => nav(`/admin/users/${u.id}`)}>
                  <td><div className="row"><Avatar name={u.full_name} size="sm" /><div><div style={{ fontWeight: 500 }}>{u.full_name}</div><div className="faint" style={{ fontSize: 12.5 }}>{u.email}</div></div></div></td>
                  <td><div className="stack-sm" style={{ gap: 3 }}>{u.memberships.map((m) => <span key={m.membership_id} className="role-chip"><i style={{ background: m.role_color }} />{m.role_name} <span className="faint">· {m.unit_name}</span></span>)}{u.memberships.length === 0 && <span className="faint">No roles</span>}</div></td>
                  <td><div className="row" style={{ gap: 6 }}>{u.totp_enabled ? <span className="tag ok">2-step</span> : <span className="tag">Password only</span>}{u.locked && <span className="tag bad"><Lock size={11} /> Locked</span>}</div></td>
                  <td className="faint">{ago(u.last_login_at)}</td>
                  <td><StatusTag status={u.status} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
      {creating && <CreateUser onClose={() => setCreating(false)} />}
    </main>
  )
}

function OneTimePassword({ password, name }: { password: string; name: string }) {
  const toast = useToast()
  return (
    <div className="stack">
      <Notice tone="warn">Share this one-time password with {name} privately. It won't be shown again, and they'll choose their own on first sign-in.</Notice>
      <div className="row" style={{ background: 'var(--surface-2)', padding: '14px 16px', borderRadius: 4 }}>
        <span className="mono" style={{ fontSize: 18, letterSpacing: '0.05em', flex: 1 }}>{password}</span>
        <button className="btn sm" onClick={() => { navigator.clipboard?.writeText(password); toast('Copied.') }}><Copy /> Copy</button>
      </div>
    </div>
  )
}

function CreateUser({ onClose }: { onClose: () => void }) {
  const { me } = useSession()
  const qc = useQueryClient()
  const { data: roles } = useRoles()
  const { data: units } = useUnits()
  const allowed = unitsWithPerm(units, me.grants, 'admin.users.manage')
  const [f, setF] = useState({ full_name: '', email: '', role_id: '', org_unit_id: '' })
  const [err, setErr] = useState('')
  const [created, setCreated] = useState<{ id: string; temporary_password: string } | null>(null)
  const m = useMutation({
    mutationFn: () => api.post<{ id: string; temporary_password: string }>('/admin/users', {
      full_name: f.full_name, email: f.email,
      memberships: f.role_id ? [{ role_id: f.role_id, org_unit_id: f.org_unit_id || allowed[0]?.id }] : [],
    }),
    onSuccess: (r) => { setCreated(r); qc.invalidateQueries({ queryKey: ['admin-users'] }) },
    onError: (e) => setErr(errorText(e)),
  })
  if (created) {
    return (
      <Modal title="Account created" onClose={onClose} footer={<Link to={`/admin/users/${created.id}`} className="btn primary" onClick={onClose}>Open profile</Link>}>
        <OneTimePassword password={created.temporary_password} name={f.full_name} />
      </Modal>
    )
  }
  return (
    <Modal title="Add a person" description="They'll get a one-time password and choose their own on first sign-in." onClose={onClose}
      footer={<><button className="btn ghost" onClick={onClose}>Cancel</button><button className="btn primary" disabled={m.isPending || !f.full_name || !f.email} onClick={() => m.mutate()}>Create account</button></>}>
      <div className="stack">
        {err && <Notice tone="bad">{err}</Notice>}
        <Field label="Full name" required><input className="input" value={f.full_name} onChange={(e) => setF({ ...f, full_name: e.target.value })} /></Field>
        <Field label="Email" required><input className="input" type="email" value={f.email} onChange={(e) => setF({ ...f, email: e.target.value })} /></Field>
        <div className="form-grid">
          <Field label="Role">
            <select className="select" value={f.role_id} onChange={(e) => setF({ ...f, role_id: e.target.value })}>
              <option value="">Choose…</option>
              {roles?.roles.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
            </select>
          </Field>
          <Field label="Where">
            <select className="select" value={f.org_unit_id} onChange={(e) => setF({ ...f, org_unit_id: e.target.value })}>
              {allowed.map((u) => <option key={u.id} value={u.id}>{unitLabel(u)}</option>)}
            </select>
          </Field>
        </div>
      </div>
    </Modal>
  )
}

interface UserDetailData {
  user: { id: string; email: string; full_name: string; status: string; must_change_password: boolean; totp_enabled: boolean; locked: boolean; failed_logins: number; profile: Record<string, unknown>; created_at: string; last_login_at: string | null; password_changed_at: string }
  memberships: { id: string; role_id: string; role_name: string; role_color: string; unit_name: string; unit_kind: string; created_at: string }[]
  relations: { id: string; full_name: string; email: string; relation: string; direction: 'guardian' | 'child' }[]
  active_sessions: number
  activity: { seq: number; at: string; action: string; ip: string | null }[]
}

export function UserDetail() {
  const { id } = useParams()
  const { me } = useSession()
  const qc = useQueryClient()
  const toast = useToast()
  const { data: roles } = useRoles()
  const { data: units } = useUnits()
  const allowed = unitsWithPerm(units, me.grants, 'admin.users.manage')
  const { data, error } = useQuery({ queryKey: ['admin-user', id], queryFn: () => api.get<UserDetailData>(`/admin/users/${id}`) })
  const [grant, setGrant] = useState({ role_id: '', org_unit_id: '' })
  const [otp, setOtp] = useState<string | null>(null)
  const [linking, setLinking] = useState(false)
  const refresh = () => { qc.invalidateQueries({ queryKey: ['admin-user', id] }); qc.invalidateQueries({ queryKey: ['admin-users'] }) }
  const onErr = (e: unknown) => toast(errorText(e), 'bad')
  const addGrant = useMutation({ mutationFn: () => api.post(`/admin/users/${id}/memberships`, { role_id: grant.role_id, org_unit_id: grant.org_unit_id || allowed[0]?.id }), onSuccess: () => { toast('Role granted.'); setGrant({ role_id: '', org_unit_id: '' }); refresh() }, onError: onErr })
  const removeGrant = useMutation({ mutationFn: (mid: string) => api.del(`/admin/memberships/${mid}`), onSuccess: () => { toast('Role removed.'); refresh() }, onError: onErr })
  const setStatus = useMutation({ mutationFn: (status: string) => api.patch(`/admin/users/${id}`, { status }), onSuccess: refresh, onError: onErr })
  const reset = useMutation({ mutationFn: () => api.post<{ temporary_password: string }>(`/admin/users/${id}/reset-password`), onSuccess: (r) => { setOtp(r.temporary_password); refresh() }, onError: onErr })
  const unlock = useMutation({ mutationFn: () => api.post(`/admin/users/${id}/unlock`), onSuccess: () => { toast('Unlocked.'); refresh() }, onError: onErr })
  const revoke = useMutation({ mutationFn: () => api.post<{ revoked: number }>(`/admin/users/${id}/revoke-sessions`), onSuccess: (r) => { toast(`Signed out of ${r.revoked} session${r.revoked === 1 ? '' : 's'}.`); refresh() }, onError: onErr })
  const unlink = useMutation({ mutationFn: (rel: { id: string; direction: string }) => rel.direction === 'guardian' ? api.del(`/admin/users/${id}/guardians/${rel.id}`) : api.del(`/admin/users/${rel.id}/guardians/${id}`), onSuccess: refresh, onError: onErr })

  if (error) return <main className="content"><Notice tone="bad">{errorText(error)}</Notice></main>
  if (!data) return <main className="content"><Spinner /></main>
  const u = data.user
  return (
    <main className="content">
      <Link to="/admin/users" className="row faint" style={{ gap: 6, fontSize: 13, marginBottom: 14 }}><ArrowLeft size={14} /> People & access</Link>
      <header className="page-head">
        <div className="row" style={{ gap: 16 }}>
          <Avatar name={u.full_name} size="lg" />
          <div>
            <h1 style={{ marginTop: 0 }}>{u.full_name}</h1>
            <p className="row wrap" style={{ gap: 8, marginTop: 6 }}><span>{u.email}</span><StatusTag status={u.status} />{u.locked && <span className="tag bad">Locked</span>}{u.must_change_password && <span className="tag warn">Must set password</span>}</p>
          </div>
        </div>
        <div className="actions">
          <button className="btn" onClick={() => reset.mutate()} disabled={reset.isPending}><KeyRound /> Reset password</button>
          {u.locked && <button className="btn" onClick={() => unlock.mutate()}><Lock /> Unlock</button>}
          <button className="btn" onClick={() => revoke.mutate()} disabled={!data.active_sessions}><LogOut /> Sign out everywhere</button>
          {u.status === 'active'
            ? <button className="btn danger" onClick={() => confirm(`Disable ${u.full_name}? They'll be signed out and unable to sign in.`) && setStatus.mutate('disabled')}>Disable</button>
            : <button className="btn primary" onClick={() => setStatus.mutate('active')}>Re-enable</button>}
        </div>
      </header>
      <div className="split">
        <div className="stack" style={{ gap: 24 }}>
          <section className="panel">
            <div className="panel-head"><h3>Roles</h3><span className="faint" style={{ fontSize: 12.5 }}>You can only grant roles whose permissions you hold yourself</span></div>
            <div className="ruled">
              {data.memberships.map((m) => (
                <div key={m.id} className="row" style={{ padding: '12px 20px' }}>
                  <span className="role-chip" style={{ fontSize: 14, color: 'var(--ink)' }}><i style={{ background: m.role_color }} />{m.role_name}</span>
                  <span className="faint">at {m.unit_name}</span>
                  <span className="faint" style={{ fontSize: 12, marginLeft: 'auto' }}>since {fmtDate(m.created_at)}</span>
                  <button className="btn ghost icon sm" aria-label="Remove role" onClick={() => confirm(`Remove ${m.role_name} at ${m.unit_name}?`) && removeGrant.mutate(m.id)}><Trash2 /></button>
                </div>
              ))}
              {data.memberships.length === 0 && <div className="panel-pad faint">No roles. This person can sign in but can't see anything.</div>}
              <div className="row wrap" style={{ padding: '12px 20px', background: 'var(--surface-2)' }}>
                <select className="select sm" style={{ width: 'auto' }} value={grant.role_id} onChange={(e) => setGrant({ ...grant, role_id: e.target.value })} aria-label="Role to grant">
                  <option value="">Grant a role…</option>
                  {roles?.roles.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
                </select>
                <span className="faint">at</span>
                <select className="select sm" style={{ width: 'auto' }} value={grant.org_unit_id} onChange={(e) => setGrant({ ...grant, org_unit_id: e.target.value })} aria-label="Where">
                  {allowed.map((x) => <option key={x.id} value={x.id}>{unitLabel(x)}</option>)}
                </select>
                <button className="btn sm primary" disabled={!grant.role_id || addGrant.isPending} onClick={() => addGrant.mutate()}><Plus /> Grant</button>
              </div>
            </div>
          </section>
          <section className="panel">
            <div className="panel-head"><h3>Family links</h3><button className="btn sm" onClick={() => setLinking(true)}><Plus /> Link guardian</button></div>
            <div className="ruled">
              {data.relations.length === 0 && <div className="panel-pad faint">No guardians or children linked. Guardians receive consent requests for their children.</div>}
              {data.relations.map((r) => (
                <div key={r.id + r.direction} className="row" style={{ padding: '12px 20px' }}>
                  <Avatar name={r.full_name} size="sm" />
                  <Link to={`/admin/users/${r.id}`} style={{ fontWeight: 500 }}>{r.full_name}</Link>
                  <span className="faint">{r.direction === 'guardian' ? `${r.relation} (guardian)` : `child (${r.full_name.split(' ')[0]}'s ${r.relation})`}</span>
                  <button className="btn ghost icon sm" style={{ marginLeft: 'auto' }} aria-label="Unlink" onClick={() => unlink.mutate(r)}><Trash2 /></button>
                </div>
              ))}
            </div>
          </section>
        </div>
        <aside className="stack" style={{ gap: 20 }}>
          <div className="panel panel-pad">
            <dl className="kv" style={{ gridTemplateColumns: '1fr auto' }}>
              <dt>Two-step sign-in</dt><dd>{u.totp_enabled ? 'On' : 'Off'}</dd>
              <dt>Active sessions</dt><dd className="num">{data.active_sessions}</dd>
              <dt>Failed attempts</dt><dd className="num">{u.failed_logins}</dd>
              <dt>Last sign-in</dt><dd>{ago(u.last_login_at)}</dd>
              <dt>Password set</dt><dd>{fmtDate(u.password_changed_at)}</dd>
              <dt>Created</dt><dd>{fmtDate(u.created_at)}</dd>
            </dl>
          </div>
          <div className="panel">
            <div className="panel-head"><h3>Recent activity</h3></div>
            <div className="ruled">
              {data.activity.length === 0 && <div className="panel-pad faint">Nothing yet.</div>}
              {data.activity.map((a) => (
                <div key={a.seq} className="row between" style={{ padding: '9px 20px', fontSize: 13 }}>
                  <span className="code">{a.action}</span><span className="faint">{ago(a.at)}</span>
                </div>
              ))}
            </div>
          </div>
        </aside>
      </div>
      {otp && <Modal title="New one-time password" onClose={() => setOtp(null)} footer={<button className="btn primary" onClick={() => setOtp(null)}>Done</button>}><OneTimePassword password={otp} name={u.full_name} /></Modal>}
      {linking && <LinkGuardian studentId={u.id} onClose={() => { setLinking(false); refresh() }} />}
    </main>
  )
}

function LinkGuardian({ studentId, onClose }: { studentId: string; onClose: () => void }) {
  const toast = useToast()
  const [q, setQ] = useState('')
  const [relation, setRelation] = useState('mother')
  const { data } = useQuery({ queryKey: ['admin-users', q, 'parent'], queryFn: () => api.get<UserRow[]>(`/admin/users${qs({ q, role: 'parent' })}`) })
  const link = useMutation({ mutationFn: (gid: string) => api.post(`/admin/users/${studentId}/guardians`, { guardian_id: gid, relation }), onSuccess: () => { toast('Linked.'); onClose() }, onError: (e) => toast(errorText(e), 'bad') })
  return (
    <Modal title="Link a guardian" description="Pick a person with the Parent role." onClose={onClose}>
      <div className="stack">
        <div className="form-grid">
          <div className="search"><Search /><input className="input" placeholder="Search parents" value={q} onChange={(e) => setQ(e.target.value)} /></div>
          <select className="select" value={relation} onChange={(e) => setRelation(e.target.value)}>
            {['mother', 'father', 'guardian', 'grandparent', 'carer'].map((r) => <option key={r}>{r}</option>)}
          </select>
        </div>
        <div className="panel ruled" style={{ maxHeight: 300, overflow: 'auto' }}>
          {data?.map((u) => (
            <button key={u.id} className="row" style={{ width: '100%', padding: '10px 14px', background: 'none', border: 0, cursor: 'pointer', textAlign: 'left' }} onClick={() => link.mutate(u.id)}>
              <Avatar name={u.full_name} size="sm" /><span style={{ fontWeight: 500 }}>{u.full_name}</span><span className="faint">{u.email}</span>
            </button>
          ))}
          {data?.length === 0 && <div className="panel-pad faint">No parents match.</div>}
        </div>
      </div>
    </Modal>
  )
}
