import { Fragment, useState } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Lock, Pencil, Plus, Trash2 } from 'lucide-react'
import { api } from '../../lib/api'
import type { Role } from '../../lib/types'
import { BoxCheck, Field, Modal, Notice, PageHead, Spinner, errorText, useToast } from '../../components/ui'
import { useRoles } from './Users'

const PALETTE = ['#7a2e2e', '#2f5d50', '#8a5a1f', '#3b5a7a', '#6b4f7a', '#55704a', '#5b6b5d', '#8c4a64', '#2e6670']

export default function Roles() {
  const { data } = useRoles()
  const qc = useQueryClient()
  const toast = useToast()
  const [editing, setEditing] = useState<Role | 'new' | null>(null)
  const toggle = useMutation({
    mutationFn: ({ role, perm, on }: { role: Role; perm: string; on: boolean }) =>
      api.patch(`/admin/roles/${role.id}`, { permissions: on ? [...role.permissions, perm] : role.permissions.filter((p) => p !== perm) }),
    onSuccess: () => qc.invalidateQueries({ queryKey: ['roles'] }),
    onError: (e) => toast(errorText(e), 'bad'),
  })
  if (!data) return <main className="content"><Spinner /></main>
  const groups = [...new Set(data.catalog.map((p) => p.group))]
  return (
    <main className="content wide" style={{ maxWidth: 1280 }}>
      <PageHead eyebrow="Control room" title="Roles & permissions" actions={<button className="btn primary" onClick={() => setEditing('new')}><Plus /> New role</button>}>
        A role is a bundle of permissions. Tick a box to grant it; changes apply the next time each person loads a page, and every change is audited.
      </PageHead>
      <div className="panel table-wrap">
        <table className="table matrix">
          <thead>
            <tr>
              <th style={{ verticalAlign: 'bottom', minWidth: 280 }}>Permission</th>
              {data.roles.map((r) => (
                <th key={r.id} className="rot">
                  <div>
                    <span style={{ display: 'inline-block', width: 8, height: 8, borderRadius: 2, background: r.color, marginBottom: 6 }} /> {r.name}
                  </div>
                </th>
              ))}
            </tr>
            <tr>
              <th />
              {data.roles.map((r) => (
                <th key={r.id} style={{ textAlign: 'center', padding: '6px 4px' }}>
                  <div className="row" style={{ justifyContent: 'center', gap: 2 }}>
                    <span className="faint num" style={{ fontSize: 11 }} title="People with this role">{r.member_count}</span>
                    <button className="btn ghost icon sm" aria-label={`Edit ${r.name}`} onClick={() => setEditing(r)}><Pencil /></button>
                  </div>
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {groups.map((g) => (
              <Fragment key={g}>
                <tr className="group"><td colSpan={data.roles.length + 1}>{g}</td></tr>
                {data.catalog.filter((p) => p.group === g).map((p) => (
                  <tr key={p.key}>
                    <td><div style={{ fontWeight: 500 }}>{p.label}</div><div className="mono faint" style={{ fontSize: 11.5 }}>{p.key}</div></td>
                    {data.roles.map((r) => {
                      const on = r.permissions.includes(p.key)
                      const locked = r.key === 'administrator' && ['admin.roles.manage', 'admin.users.manage', 'admin.settings.manage'].includes(p.key)
                      return (
                        <td key={r.id} className="cell">
                          {locked ? <Lock size={14} style={{ margin: '0 auto', color: 'var(--ink-3)' }} aria-label="Always on" /> : (
                            <BoxCheck on={on} label={`${p.label} for ${r.name}`} onChange={(v) => toggle.mutate({ role: r, perm: p.key, on: v })} />
                          )}
                        </td>
                      )
                    })}
                  </tr>
                ))}
              </Fragment>
            ))}
          </tbody>
        </table>
      </div>
      {editing && <RoleModal role={editing === 'new' ? null : editing} onClose={() => setEditing(null)} />}
    </main>
  )
}

function RoleModal({ role, onClose }: { role: Role | null; onClose: () => void }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [f, setF] = useState({ key: role?.key ?? '', name: role?.name ?? '', description: role?.description ?? '', color: role?.color ?? PALETTE[6] })
  const [err, setErr] = useState('')
  const save = useMutation({
    mutationFn: () => (role ? api.patch(`/admin/roles/${role.id}`, { name: f.name, description: f.description, color: f.color }) : api.post('/admin/roles', { ...f, permissions: [] })),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['roles'] }); toast(role ? 'Role updated.' : 'Role created. Now tick its permissions.'); onClose() },
    onError: (e) => setErr(errorText(e)),
  })
  const del = useMutation({
    mutationFn: () => api.del(`/admin/roles/${role!.id}`),
    onSuccess: () => { qc.invalidateQueries({ queryKey: ['roles'] }); toast('Role deleted.'); onClose() },
    onError: (e) => setErr(errorText(e)),
  })
  return (
    <Modal title={role ? `Edit ${role.name}` : 'New role'} onClose={onClose}
      footer={<>
        {role && !role.is_system && <button className="btn danger" style={{ marginRight: 'auto' }} onClick={() => del.mutate()}><Trash2 /> Delete</button>}
        <button className="btn ghost" onClick={onClose}>Cancel</button>
        <button className="btn primary" disabled={!f.name || (!role && !f.key) || save.isPending} onClick={() => save.mutate()}>Save</button>
      </>}>
      <div className="stack">
        {err && <Notice tone="bad">{err}</Notice>}
        <Field label="Name" required><input className="input" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value, key: role ? f.key : e.target.value.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '').slice(0, 32) })} placeholder="e.g. Librarian" /></Field>
        {!role && <Field label="Key" hint="Used in workflows. Lowercase, can't be changed later."><input className="input mono" value={f.key} onChange={(e) => setF({ ...f, key: e.target.value })} /></Field>}
        <Field label="Description"><input className="input" value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} /></Field>
        <Field label="Colour">
          <div className="row wrap" style={{ gap: 6 }}>
            {PALETTE.map((c) => (
              <button key={c} type="button" aria-label={c} onClick={() => setF({ ...f, color: c })} style={{ width: 26, height: 26, borderRadius: 4, background: c, border: f.color === c ? '2px solid var(--ink)' : '2px solid transparent', cursor: 'pointer' }} />
            ))}
          </div>
        </Field>
        {role?.is_system && <p className="faint" style={{ margin: 0, fontSize: 12.5 }}>Built-in roles can be renamed and re-permissioned but not deleted.</p>}
      </div>
    </Modal>
  )
}
