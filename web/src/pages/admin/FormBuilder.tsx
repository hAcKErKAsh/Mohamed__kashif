import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowDown, ArrowUp, Eye, Plus, Trash2 } from 'lucide-react'
import { api } from '../../lib/api'
import type { FieldDef } from '../../lib/types'
import { FormRenderer } from '../../components/FormRenderer'
import { Empty, Field, Notice, PageHead, Spinner, StatusTag, Toggle, ago, errorText, useToast } from '../../components/ui'
import { useRoles } from './Users'

interface FormDef { id: string; key: string; name: string; description: string; fields: FieldDef[]; workflow_key: string | null; allowed_roles: string[]; active: boolean; submissions: number; updated_at: string }
interface WorkflowGroup { key: string; versions: { status: string; name: string }[] }

const TYPES: [FieldDef['type'], string][] = [['text', 'Short text'], ['textarea', 'Long text'], ['number', 'Number'], ['date', 'Date'], ['select', 'Choice'], ['checkbox', 'Tick box'], ['email', 'Email']]

const blank = (): FormDef => ({ id: '', key: '', name: 'Untitled form', description: '', fields: [{ key: 'details', label: 'Details', type: 'textarea', required: true }], workflow_key: null, allowed_roles: [], active: true, submissions: 0, updated_at: '' })

export default function FormBuilder() {
  const { data } = useQuery({ queryKey: ['admin-forms'], queryFn: () => api.get<FormDef[]>('/admin/forms') })
  const [editing, setEditing] = useState<FormDef | null>(null)
  if (!data) return <main className="content"><Spinner /></main>
  if (editing) return <Editor initial={editing} onDone={() => setEditing(null)} />
  return (
    <main className="content">
      <PageHead eyebrow="Control room" title="Form builder" actions={<button className="btn primary" onClick={() => setEditing(blank())}><Plus /> New form</button>}>
        Build request forms with no code: choose the fields, who may use it, and which workflow approves it.
      </PageHead>
      <div className="panel table-wrap">
        {data.length === 0 ? <Empty title="No forms yet" /> : (
          <table className="table">
            <thead><tr><th>Form</th><th>Fields</th><th>Approved by</th><th>Who can use it</th><th>Used</th><th>Status</th></tr></thead>
            <tbody>
              {data.map((f) => (
                <tr key={f.id} className="clickable" onClick={() => setEditing(f)}>
                  <td><div style={{ fontWeight: 500 }}>{f.name}</div><div className="mono faint" style={{ fontSize: 11.5 }}>{f.key}</div></td>
                  <td className="num">{f.fields.length}</td>
                  <td>{f.workflow_key ? <span className="code">{f.workflow_key}</span> : <span className="faint">Recorded only</span>}</td>
                  <td className="muted">{f.allowed_roles.length ? f.allowed_roles.join(', ') : 'Everyone'}</td>
                  <td className="faint">{f.submissions} · {ago(f.updated_at)}</td>
                  <td><StatusTag status={f.active ? 'active' : 'disabled'} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </main>
  )
}

function Editor({ initial, onDone }: { initial: FormDef; onDone: () => void }) {
  const qc = useQueryClient()
  const toast = useToast()
  const { data: roles } = useRoles()
  const { data: workflows } = useQuery({ queryKey: ['workflows'], queryFn: () => api.get<WorkflowGroup[]>('/workflows') })
  const [f, setF] = useState<FormDef>(structuredClone(initial))
  const [preview, setPreview] = useState<Record<string, unknown>>({})
  const [err, setErr] = useState('')
  const isNew = !initial.id
  const save = useMutation({
    mutationFn: () => {
      const body = { key: f.key, name: f.name, description: f.description, fields: f.fields, workflow_key: f.workflow_key, allowed_roles: f.allowed_roles, active: f.active }
      return isNew ? api.post('/admin/forms', body) : api.put(`/admin/forms/${f.id}`, body)
    },
    onSuccess: () => { toast('Form saved.'); qc.invalidateQueries({ queryKey: ['admin-forms'] }); qc.invalidateQueries({ queryKey: ['forms'] }); onDone() },
    onError: (e) => setErr(errorText(e)),
  })
  const setField = (i: number, patch: Partial<FieldDef>) => setF({ ...f, fields: f.fields.map((x, j) => (j === i ? { ...x, ...patch } : x)) })
  const move = (i: number, d: number) => {
    const next = [...f.fields]
    const [x] = next.splice(i, 1)
    next.splice(i + d, 0, x)
    setF({ ...f, fields: next })
  }
  return (
    <main className="content wide" style={{ maxWidth: 1280 }}>
      <PageHead eyebrow={isNew ? 'New form' : 'Edit form'} title={f.name || 'Untitled form'} actions={<><button className="btn ghost" onClick={onDone}>Cancel</button><button className="btn primary" onClick={() => save.mutate()} disabled={save.isPending}>Save form</button></>} />
      {err && <div style={{ marginBottom: 16 }}><Notice tone="bad">{err}</Notice></div>}
      <div className="grid-2" style={{ alignItems: 'start' }}>
        <div className="stack" style={{ gap: 20 }}>
          <section className="panel panel-pad stack">
            <div className="form-grid">
              <Field label="Name" required><input className="input" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value, key: isNew ? e.target.value.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '').slice(0, 48) : f.key })} /></Field>
              <Field label="Key"><input className="input mono" value={f.key} disabled={!isNew} onChange={(e) => setF({ ...f, key: e.target.value })} /></Field>
              <Field label="Description" className="full"><input className="input" value={f.description} onChange={(e) => setF({ ...f, description: e.target.value })} /></Field>
              <Field label="Approval workflow" hint="Its rules can use any field key below.">
                <select className="select" value={f.workflow_key ?? ''} onChange={(e) => setF({ ...f, workflow_key: e.target.value || null })}>
                  <option value="">None — just record it</option>
                  {workflows?.map((w) => <option key={w.key} value={w.key}>{w.versions[0]?.name} ({w.key}){w.versions.some((v) => v.status === 'published') ? '' : ' — not published'}</option>)}
                </select>
              </Field>
              <div className="field"><label>Accepting submissions</label><div className="row" style={{ height: 36 }}><Toggle on={f.active} onChange={(v) => setF({ ...f, active: v })} label="Active" /><span className="muted">{f.active ? 'Open' : 'Closed'}</span></div></div>
            </div>
            <Field label="Who can use it" hint="Leave all unticked for everyone with the forms permission.">
              <div className="row wrap" style={{ gap: 14 }}>
                {roles?.roles.map((r) => (
                  <label key={r.id} className="check"><input type="checkbox" checked={f.allowed_roles.includes(r.key)} onChange={(e) => setF({ ...f, allowed_roles: e.target.checked ? [...f.allowed_roles, r.key] : f.allowed_roles.filter((x) => x !== r.key) })} /> {r.name}</label>
                ))}
              </div>
            </Field>
          </section>
          <section className="panel">
            <div className="panel-head"><h3>Fields</h3><button className="btn sm" onClick={() => setF({ ...f, fields: [...f.fields, { key: `field_${f.fields.length + 1}`, label: 'New question', type: 'text' }] })}><Plus /> Add field</button></div>
            <div className="ruled">
              {f.fields.map((x, i) => (
                <div key={i} className="panel-pad stack-sm">
                  <div className="row" style={{ gap: 6 }}>
                    <input className="input sm" style={{ flex: 2 }} value={x.label} onChange={(e) => setField(i, { label: e.target.value })} aria-label="Question" />
                    <select className="select sm" style={{ flex: 1 }} value={x.type} onChange={(e) => setField(i, { type: e.target.value as FieldDef['type'], options: e.target.value === 'select' ? x.options ?? ['Option A', 'Option B'] : undefined })} aria-label="Type">
                      {TYPES.map(([k, l]) => <option key={k} value={k}>{l}</option>)}
                    </select>
                    <button className="btn ghost icon sm" disabled={i === 0} onClick={() => move(i, -1)} aria-label="Move up"><ArrowUp /></button>
                    <button className="btn ghost icon sm" disabled={i === f.fields.length - 1} onClick={() => move(i, 1)} aria-label="Move down"><ArrowDown /></button>
                    <button className="btn ghost icon sm" onClick={() => setF({ ...f, fields: f.fields.filter((_, j) => j !== i) })} aria-label="Remove field"><Trash2 /></button>
                  </div>
                  <div className="row wrap" style={{ gap: 10, fontSize: 13 }}>
                    <span className="faint">key</span>
                    <input className="input sm mono" style={{ width: 150 }} value={x.key} onChange={(e) => setField(i, { key: e.target.value.toLowerCase().replace(/[^a-z0-9_]/g, '') })} aria-label="Field key" />
                    <label className="check"><input type="checkbox" checked={!!x.required} onChange={(e) => setField(i, { required: e.target.checked })} /> Required</label>
                    {x.type === 'number' && (<>
                      <input className="input sm" style={{ width: 80 }} type="number" placeholder="min" value={x.min ?? ''} onChange={(e) => setField(i, { min: e.target.value === '' ? undefined : Number(e.target.value) })} />
                      <input className="input sm" style={{ width: 80 }} type="number" placeholder="max" value={x.max ?? ''} onChange={(e) => setField(i, { max: e.target.value === '' ? undefined : Number(e.target.value) })} />
                    </>)}
                  </div>
                  {x.type === 'select' && <input className="input sm" placeholder="Options, separated by commas" value={x.options?.join(', ') ?? ''} onChange={(e) => setField(i, { options: e.target.value.split(',').map((s) => s.trim()).filter(Boolean) })} />}
                  <input className="input sm" placeholder="Help text (optional)" value={x.help ?? ''} onChange={(e) => setField(i, { help: e.target.value || undefined })} />
                </div>
              ))}
            </div>
          </section>
        </div>
        <section className="panel" style={{ position: 'sticky', top: 80 }}>
          <div className="panel-head"><h3 className="row" style={{ gap: 8 }}><Eye size={15} /> Live preview</h3></div>
          <div className="panel-pad stack">
            <div className="serif" style={{ fontSize: 22 }}>{f.name}</div>
            {f.description && <p className="muted" style={{ margin: 0 }}>{f.description}</p>}
            <FormRenderer fields={f.fields} value={preview} onChange={setPreview} />
          </div>
        </section>
      </div>
    </main>
  )
}
