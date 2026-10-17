import { useEffect, useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ChevronRight, FolderTree, Plus, Trash2 } from 'lucide-react'
import { api, qs } from '../../lib/api'
import type { Unit } from '../../lib/types'
import { unitLabel, useUnits } from '../../lib/units'
import { Avatar, Field, Modal, Notice, PageHead, Spinner, errorText, useToast } from '../../components/ui'

export default function Org() {
  const { data: units } = useUnits()
  const { data: kinds } = useQuery({ queryKey: ['settings'], queryFn: () => api.get<Record<string, unknown>>('/admin/settings'), select: (s) => (s.org_kinds as string[]) ?? [] })
  const [sel, setSel] = useState<string | null>(null)
  const [adding, setAdding] = useState(false)
  useEffect(() => { if (!sel && units?.length) setSel(units[0].id) }, [units, sel])
  const children = useMemo(() => {
    const m = new Map<string | null, Unit[]>()
    for (const u of units ?? []) m.set(u.parent_id, [...(m.get(u.parent_id) ?? []), u])
    for (const v of m.values()) v.sort((a, b) => a.sort_order - b.sort_order || a.name.localeCompare(b.name))
    return m
  }, [units])
  if (!units) return <main className="content"><Spinner /></main>
  const selected = units.find((u) => u.id === sel)

  const render = (parent: string | null) => (
    <ul className="tree">
      {(children.get(parent) ?? []).map((u) => (
        <li key={u.id}>
          <div className={`tree-row ${sel === u.id ? 'on' : ''}`} onClick={() => setSel(u.id)} role="treeitem" aria-selected={sel === u.id} tabIndex={0} onKeyDown={(e) => e.key === 'Enter' && setSel(u.id)}>
            {children.has(u.id) ? <ChevronRight style={{ transform: 'rotate(90deg)' }} /> : <span style={{ width: 14 }} />}
            <span>{u.name}</span>
            <span className="kind">{u.kind}</span>
          </div>
          {children.has(u.id) && render(u.id)}
        </li>
      ))}
    </ul>
  )

  return (
    <main className="content wide" style={{ maxWidth: 1280 }}>
      <PageHead eyebrow="Control room" title="Organisation" actions={selected && <button className="btn primary" onClick={() => setAdding(true)}><Plus /> Add inside {selected.name}</button>}>
        The tree everything hangs from. Roles granted on a branch apply to all of it; approvals look upward through it to find the right person.
      </PageHead>
      <div className="split-wide">
        <div className="panel panel-pad" role="tree" aria-label="Organisation tree">{render(null)}</div>
        {selected && <UnitPanel key={selected.id} unit={selected} units={units} kinds={kinds ?? []} hasChildren={children.has(selected.id)} onDeleted={() => setSel(null)} />}
      </div>
      {adding && selected && <AddUnit parent={selected} kinds={kinds ?? []} onClose={() => setAdding(false)} onCreated={setSel} />}
    </main>
  )
}

function UnitPanel({ unit, units, kinds, hasChildren, onDeleted }: { unit: Unit; units: Unit[]; kinds: string[]; hasChildren: boolean; onDeleted: () => void }) {
  const qc = useQueryClient()
  const toast = useToast()
  const [f, setF] = useState({ name: unit.name, kind: unit.kind, code: unit.code })
  const [parent, setParent] = useState(unit.parent_id ?? '')
  const done = (msg: string) => () => { toast(msg); qc.invalidateQueries({ queryKey: ['org-tree'] }) }
  const onError = (e: unknown) => toast(errorText(e), 'bad')
  const save = useMutation({ mutationFn: () => api.patch(`/org/units/${unit.id}`, f), onSuccess: done('Saved.'), onError })
  const move = useMutation({ mutationFn: () => api.post(`/org/units/${unit.id}/move`, { parent_id: parent }), onSuccess: done('Moved. Everything inside moved with it.'), onError })
  const del = useMutation({ mutationFn: () => api.del(`/org/units/${unit.id}`), onSuccess: () => { done('Deleted.')(); onDeleted() }, onError })
  const { data: members } = useQuery({ queryKey: ['admin-users', 'unit', unit.id], queryFn: () => api.get<{ id: string; full_name: string; memberships: { role_name: string; unit_id: string }[] }[]>(`/admin/users${qs({ unit: unit.id })}`) })
  const isRoot = !unit.parent_id
  const parentOptions = units.filter((u) => u.id !== unit.id && !(u.path + '.').startsWith(unit.path + '.'))
  return (
    <div className="stack" style={{ gap: 20 }}>
      <section className="panel">
        <div className="panel-head"><h3 className="row" style={{ gap: 8 }}><FolderTree size={15} /> {unit.name}</h3><span className="faint mono" style={{ fontSize: 12 }}>{unit.code}</span></div>
        <div className="panel-pad form-grid">
          <Field label="Name" className="full"><input className="input" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value })} /></Field>
          <Field label="Kind" hint="Approval rules can target the nearest unit of a kind.">
            <select className="select" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })}>{[...new Set([...kinds, f.kind])].map((k) => <option key={k}>{k}</option>)}</select>
          </Field>
          <Field label="Code"><input className="input mono" value={f.code} onChange={(e) => setF({ ...f, code: e.target.value })} /></Field>
        </div>
        <div className="panel-foot">
          {!isRoot && <button className="btn danger" style={{ marginRight: 'auto' }} disabled={hasChildren} title={hasChildren ? 'Remove what is inside first' : ''} onClick={() => confirm(`Delete ${unit.name}?`) && del.mutate()}><Trash2 /> Delete</button>}
          <button className="btn primary" disabled={save.isPending} onClick={() => save.mutate()}>Save</button>
        </div>
      </section>
      {!isRoot && (
        <section className="panel panel-pad row wrap">
          <span className="label">Move under</span>
          <select className="select" style={{ width: 'auto', flex: 1 }} value={parent} onChange={(e) => setParent(e.target.value)}>{parentOptions.map((u) => <option key={u.id} value={u.id}>{unitLabel(u)}</option>)}</select>
          <button className="btn" disabled={parent === unit.parent_id || move.isPending} onClick={() => move.mutate()}>Move</button>
        </section>
      )}
      <section className="panel">
        <div className="panel-head"><h3>People in this branch</h3><span className="faint" style={{ fontSize: 12.5 }}>{members?.length ?? 0}</span></div>
        <div className="ruled" style={{ maxHeight: 420, overflow: 'auto' }}>
          {members?.length === 0 && <div className="panel-pad faint">No one is assigned here yet.</div>}
          {members?.map((m) => (
            <Link key={m.id} to={`/admin/users/${m.id}`} className="row" style={{ padding: '10px 20px' }}>
              <Avatar name={m.full_name} size="sm" />
              <span style={{ fontWeight: 500 }}>{m.full_name}</span>
              <span className="faint" style={{ marginLeft: 'auto', fontSize: 12.5 }}>{m.memberships.map((x) => x.role_name).filter((v, i, a) => a.indexOf(v) === i).join(', ')}</span>
            </Link>
          ))}
        </div>
      </section>
    </div>
  )
}

function AddUnit({ parent, kinds, onClose, onCreated }: { parent: Unit; kinds: string[]; onClose: () => void; onCreated: (id: string) => void }) {
  const qc = useQueryClient()
  const [f, setF] = useState({ name: '', code: '', kind: kinds[Math.min(kinds.indexOf(parent.kind) + 1, kinds.length - 1)] ?? 'section' })
  const [err, setErr] = useState('')
  const m = useMutation({
    mutationFn: () => api.post<{ id: string }>('/org/units', { ...f, parent_id: parent.id }),
    onSuccess: (r) => { qc.invalidateQueries({ queryKey: ['org-tree'] }); onCreated(r.id); onClose() },
    onError: (e) => setErr(errorText(e)),
  })
  return (
    <Modal title={`Add inside ${parent.name}`} onClose={onClose} footer={<><button className="btn ghost" onClick={onClose}>Cancel</button><button className="btn primary" disabled={!f.name || !f.code || m.isPending} onClick={() => m.mutate()}>Add</button></>}>
      <div className="stack">
        {err && <Notice tone="bad">{err}</Notice>}
        <Field label="Name" required><input className="input" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value, code: f.code || '' })} placeholder="e.g. Grade 6" /></Field>
        <div className="form-grid">
          <Field label="Code" required hint="Short and unique among siblings."><input className="input mono" value={f.code} onChange={(e) => setF({ ...f, code: e.target.value })} placeholder="G6" /></Field>
          <Field label="Kind"><select className="select" value={f.kind} onChange={(e) => setF({ ...f, kind: e.target.value })}>{kinds.map((k) => <option key={k}>{k}</option>)}</select></Field>
        </div>
      </div>
    </Modal>
  )
}
