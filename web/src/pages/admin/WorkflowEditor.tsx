import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowLeft, FlaskConical, Plus, Rocket, Save, Trash2, X } from 'lucide-react'
import { api } from '../../lib/api'
import type { ApproverRule, Cond, FlowEdge, FlowNode, Graph, NodeType } from '../../lib/types'
import { useSession } from '../../lib/session'
import { unitLabel, useUnits } from '../../lib/units'
import { FlowDiagram } from '../../components/FlowDiagram'
import { Field, Notice, PageHead, Spinner, StatusTag, errorText, useToast } from '../../components/ui'
import { useRoles } from './Users'

interface Def { id: string; key: string; version: number; name: string; description: string; graph: Graph; status: string }
interface TraceStep { node_id: string; type: NodeType; name: string; approvers?: string[]; mode?: string; chose?: string; outcome?: string; fallback?: boolean }

const OPS: [string, string][] = [['eq', 'is'], ['ne', 'is not'], ['gt', '>'], ['gte', '≥'], ['lt', '<'], ['lte', '≤'], ['in', 'is one of'], ['contains', 'contains'], ['exists', 'is filled in'], ['empty', 'is empty']]
const FIELD_HINTS = ['days', 'type', 'reason', 'budget', 'amount', 'audience_kind', 'kind', 'requester.roles', 'unit.kind', 'unit.depth']

const STEP_TYPES: [NodeType, string, string][] = [
  ['approval', 'Approval stage', 'People decide'],
  ['condition', 'Decision', 'Route by the request’s details'],
  ['parallel', 'Parallel split', 'Ask several stages at once'],
  ['join', 'Wait for all', 'Continue when parallel stages finish'],
  ['end', 'End', 'Approved or declined'],
]

function newId(g: Graph, base: string) {
  let i = 1
  while (g.nodes.some((n) => n.id === `${base}_${i}`)) i++
  return `${base}_${i}`
}

export default function WorkflowEditor() {
  const { id } = useParams()
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const { data: def } = useQuery({ queryKey: ['workflow', id], queryFn: () => api.get<Def>(`/workflows/${id}`) })
  const [g, setG] = useState<Graph | null>(null)
  const [meta, setMeta] = useState({ name: '', description: '' })
  const [sel, setSel] = useState<string | null>(null)
  const [errors, setErrors] = useState<string[]>([])
  const [dirty, setDirty] = useState(false)
  const [simOpen, setSimOpen] = useState(false)
  useEffect(() => {
    if (def) { setG(structuredClone(def.graph)); setMeta({ name: def.name, description: def.description }); setDirty(false); setErrors([]) }
  }, [def])

  const readOnly = def?.status !== 'draft'
  const update = (fn: (g: Graph) => void) => {
    if (readOnly) return
    setG((old) => { const next = structuredClone(old!); fn(next); return next })
    setDirty(true)
  }

  const save = useMutation({
    mutationFn: () => api.put(`/workflows/${id}`, { ...meta, graph: g }),
    onSuccess: () => { setDirty(false); toast('Draft saved.'); qc.invalidateQueries({ queryKey: ['workflows'] }) },
    onError: (e) => toast(errorText(e), 'bad'),
  })
  const publish = useMutation({
    mutationFn: async () => {
      if (dirty) await api.put(`/workflows/${id}`, { ...meta, graph: g })
      return api.post<{ ok: boolean; errors?: string[] }>(`/workflows/${id}/publish`)
    },
    onSuccess: (r) => {
      if (!r.ok) { setErrors(r.errors ?? []); toast('Fix the issues below before publishing.', 'bad'); return }
      setErrors([]); toast('Published. New requests will use this version.')
      qc.invalidateQueries({ queryKey: ['workflow', id] }); qc.invalidateQueries({ queryKey: ['workflows'] })
    },
    onError: (e) => toast(errorText(e), 'bad'),
  })
  const newVersion = useMutation({
    mutationFn: () => api.post<{ id: string }>('/workflows', { key: def!.key, name: meta.name, description: meta.description, graph: g }),
    onSuccess: (r) => { qc.invalidateQueries({ queryKey: ['workflows'] }); nav(`/admin/workflows/${r.id}`) },
    onError: (e) => toast(errorText(e), 'bad'),
  })

  if (!def || !g) return <main className="content"><Spinner /></main>
  const node = g.nodes.find((n) => n.id === sel) ?? null

  const addStep = (type: NodeType) => update((d) => {
    const id = newId(d, type === 'approval' ? 'stage' : type)
    const n: FlowNode = { id, type, name: STEP_TYPES.find((t) => t[0] === type)![1] }
    if (type === 'approval') { n.approvers = { kind: 'role', role: 'teacher', search: 'nearest' }; n.mode = 'any' }
    if (type === 'end') n.outcome = 'approved'
    d.nodes.push(n)
    if (sel && d.nodes.find((x) => x.id === sel)?.type !== 'end') {
      const from = d.nodes.find((x) => x.id === sel)!
      d.edges.push({ from: sel, to: id, ...(from.type === 'approval' ? { on: 'approved' as const } : {}), ...(from.type === 'condition' && !d.edges.some((e) => e.from === sel && e.default) ? { default: true } : {}) })
    }
    setSel(id)
  })

  return (
    <main className="content wide" style={{ maxWidth: 1360 }}>
      <Link to="/admin/workflows" className="row faint" style={{ gap: 6, fontSize: 13, marginBottom: 14 }}><ArrowLeft size={14} /> Workflows</Link>
      <PageHead
        eyebrow={`${def.key} · version ${def.version}`}
        title={<span className="row" style={{ gap: 12 }}>{meta.name}<StatusTag status={def.status} /></span>}
        actions={<>
          <button className="btn" onClick={() => setSimOpen((s) => !s)}><FlaskConical /> Test run</button>
          {readOnly ? (
            <button className="btn primary" onClick={() => newVersion.mutate()} disabled={newVersion.isPending}><Plus /> Edit as new version</button>
          ) : (<>
            <button className="btn" onClick={() => save.mutate()} disabled={!dirty || save.isPending}><Save /> Save draft</button>
            <button className="btn primary" onClick={() => publish.mutate()} disabled={publish.isPending}><Rocket /> Publish</button>
          </>)}
        </>}
      >
        {readOnly ? 'Published versions are frozen so that every decision can be traced to the exact rules in force. Edit as a new version to make changes.' : 'Click a step to configure it. Add steps after the selected one.'}
      </PageHead>

      {errors.length > 0 && <div style={{ marginBottom: 16 }}><Notice tone="bad"><strong>This workflow can't be published yet:</strong><ul style={{ margin: '6px 0 0', paddingLeft: 18 }}>{errors.map((e, i) => <li key={i}>{e}</li>)}</ul></Notice></div>}

      {!readOnly && (
        <div className="row wrap" style={{ marginBottom: 12 }}>
          <span className="faint" style={{ fontSize: 13 }}>Add after {node ? `“${node.name ?? node.id}”` : 'nothing selected'}:</span>
          {STEP_TYPES.map(([t, label, hint]) => <button key={t} className="btn sm" title={hint} onClick={() => addStep(t)}><Plus /> {label}</button>)}
        </div>
      )}

      <FlowDiagram graph={g} selected={sel} onSelect={setSel} />

      {simOpen && <Simulator graph={g} onClose={() => setSimOpen(false)} />}

      <div className="grid-2" style={{ marginTop: 20, alignItems: 'start' }}>
        {node ? (
          <StepInspector key={node.id} node={node} graph={g} readOnly={readOnly} update={update} onDelete={() => { update((d) => { d.nodes = d.nodes.filter((n) => n.id !== node.id); d.edges = d.edges.filter((e) => e.from !== node.id && e.to !== node.id) }); setSel(null) }} />
        ) : (
          <div className="panel panel-pad stack">
            <Field label="Workflow name"><input className="input" value={meta.name} disabled={readOnly} onChange={(e) => { setMeta({ ...meta, name: e.target.value }); setDirty(true) }} /></Field>
            <Field label="Description"><textarea className="textarea" value={meta.description} disabled={readOnly} onChange={(e) => { setMeta({ ...meta, description: e.target.value }); setDirty(true) }} /></Field>
            <Field label="If a stage has nobody to ask" hint="For example, a section with no teacher assigned yet.">
              <select className="select" value={g.fallback ?? 'admins'} disabled={readOnly} onChange={(e) => update((d) => { d.fallback = e.target.value as 'admins' | 'skip' })}>
                <option value="admins">Send it to workflow administrators</option>
                <option value="skip">Skip that stage automatically</option>
              </select>
            </Field>
          </div>
        )}
        {node && <Connections node={node} graph={g} readOnly={readOnly} update={update} onSelect={setSel} />}
      </div>
    </main>
  )
}

function StepInspector({ node, graph, readOnly, update, onDelete }: { node: FlowNode; graph: Graph; readOnly: boolean; update: (fn: (g: Graph) => void) => void; onDelete: () => void }) {
  const { data: roles } = useRoles()
  const set = (fn: (n: FlowNode) => void) => update((g) => fn(g.nodes.find((n) => n.id === node.id)!))
  const rule = node.approvers
  const ruleKind = rule?.kind ?? 'role'
  return (
    <div className="panel">
      <div className="panel-head">
        <h3>{STEP_TYPES.find((t) => t[0] === node.type)?.[1] ?? 'Start'}</h3>
        {!readOnly && node.type !== 'start' && <button className="btn ghost sm danger" onClick={onDelete}><Trash2 /> Remove</button>}
      </div>
      <div className="panel-pad stack">
        <Field label="Label"><input className="input" value={node.name ?? ''} disabled={readOnly} onChange={(e) => set((n) => { n.name = e.target.value })} /></Field>
        {node.type === 'end' && (
          <Field label="Outcome">
            <div className="segmented">
              {(['approved', 'rejected'] as const).map((o) => <button key={o} disabled={readOnly} className={node.outcome === o ? 'on' : ''} onClick={() => set((n) => { n.outcome = o })}>{o === 'approved' ? 'Approved' : 'Declined'}</button>)}
            </div>
          </Field>
        )}
        {node.type === 'approval' && (<>
          <Field label="Who decides">
            <select className="select" value={ruleKind} disabled={readOnly} onChange={(e) => set((n) => {
              const k = e.target.value
              n.approvers = k === 'guardians' ? { kind: 'guardians' } : k === 'roles' ? { kind: 'roles', roles: [], search: 'all_ancestors' } : k === 'users' ? { kind: 'users', ids: [] } : { kind: 'role', role: roles?.roles[0]?.key ?? 'teacher', search: 'nearest' }
            })}>
              <option value="role">People with a role</option>
              <option value="roles">A panel drawn from several roles</option>
              <option value="guardians">The requester’s guardians</option>
              <option value="users" disabled>Named people (via API)</option>
            </select>
          </Field>
          {rule?.kind === 'role' && (
            <div className="form-grid">
              <Field label="Role"><select className="select" value={rule.role} disabled={readOnly} onChange={(e) => set((n) => { (n.approvers as Extract<ApproverRule, { kind: 'role' }>).role = e.target.value })}>{roles?.roles.map((r) => <option key={r.id} value={r.key}>{r.name}</option>)}</select></Field>
              <SearchSelect value={rule.search ?? 'nearest'} disabled={readOnly} onChange={(v) => set((n) => { (n.approvers as { search?: string }).search = v })} />
            </div>
          )}
          {rule?.kind === 'roles' && (<>
            <Field label="Roles on the panel">
              <div className="row wrap" style={{ gap: 12 }}>
                {roles?.roles.map((r) => (
                  <label key={r.id} className="check"><input type="checkbox" disabled={readOnly} checked={rule.roles.includes(r.key)} onChange={(e) => set((n) => { const a = n.approvers as Extract<ApproverRule, { kind: 'roles' }>; a.roles = e.target.checked ? [...a.roles, r.key] : a.roles.filter((x) => x !== r.key) })} /> {r.name}</label>
                ))}
              </div>
            </Field>
            <SearchSelect value={rule.search ?? 'all_ancestors'} disabled={readOnly} onChange={(v) => set((n) => { (n.approvers as { search?: string }).search = v })} />
          </>)}
          <div className="form-grid">
            <Field label="How votes count">
              <select className="select" value={node.mode ?? 'any'} disabled={readOnly} onChange={(e) => set((n) => { n.mode = e.target.value as FlowNode['mode']; if (n.mode === 'quorum' && !n.quorum) n.quorum = 2 })}>
                <option value="any">Any one person decides</option>
                <option value="all">Everyone must approve</option>
                <option value="quorum">A minimum number must approve</option>
              </select>
            </Field>
            {node.mode === 'quorum' && <Field label="Approvals needed"><input className="input" type="number" min={1} value={node.quorum ?? 2} disabled={readOnly} onChange={(e) => set((n) => { n.quorum = Number(e.target.value) })} /></Field>}
          </div>
          <div className="form-grid">
            <Field label="Time limit (hours)" hint="Leave empty for none."><input className="input" type="number" min={1} value={node.sla_hours ?? ''} disabled={readOnly} onChange={(e) => set((n) => { n.sla_hours = e.target.value ? Number(e.target.value) : null })} /></Field>
            <Field label="If late, also ask" hint="Nearest holder of this role.">
              <select className="select" value={node.escalate_to ?? ''} disabled={readOnly || !node.sla_hours} onChange={(e) => set((n) => { n.escalate_to = e.target.value || null })}>
                <option value="">No one</option>
                {roles?.roles.map((r) => <option key={r.id} value={r.key}>{r.name}</option>)}
              </select>
            </Field>
          </div>
          <p className="faint" style={{ margin: 0, fontSize: 12.5 }}>The requester is never asked to approve their own request. Every approver can also send it back for changes.</p>
        </>)}
        {node.type === 'condition' && <p className="muted" style={{ margin: 0, fontSize: 13.5 }}>A decision checks each path's rule in order and takes the first that matches; otherwise it takes the default path. Set the rules under <strong>Paths</strong>.</p>}
        {node.type === 'parallel' && <p className="muted" style={{ margin: 0, fontSize: 13.5 }}>Every path out of here starts at once. Bring them back together with a “Wait for all” step.</p>}
        {node.type === 'join' && <p className="muted" style={{ margin: 0, fontSize: 13.5 }}>Continues once every parallel stage has finished. If any of them declines, the whole request is declined.</p>}
        <div className="faint mono" style={{ fontSize: 11.5 }}>id: {node.id} · {graph.edges.filter((e) => e.to === node.id).length} incoming</div>
      </div>
    </div>
  )
}

function SearchSelect({ value, onChange, disabled }: { value: string; onChange: (v: string) => void; disabled?: boolean }) {
  return (
    <Field label="Look for them">
      <select className="select" value={value} disabled={disabled} onChange={(e) => onChange(e.target.value)}>
        <option value="nearest">Closest to the request</option>
        <option value="all_ancestors">Anywhere above the request</option>
        <option value="kind:section">At the section</option>
        <option value="kind:grade">At the grade</option>
        <option value="kind:wing">At the wing</option>
        <option value="kind:school">At the school</option>
      </select>
    </Field>
  )
}

function Connections({ node, graph, readOnly, update, onSelect }: { node: FlowNode; graph: Graph; readOnly: boolean; update: (fn: (g: Graph) => void) => void; onSelect: (id: string) => void }) {
  const outs = graph.edges.map((e, i) => ({ e, i })).filter(({ e }) => e.from === node.id)
  const targets = graph.nodes.filter((n) => n.id !== node.id && n.type !== 'start')
  const setEdge = (i: number, fn: (e: FlowEdge) => void) => update((g) => fn(g.edges[i]))
  if (node.type === 'end') return <div className="panel panel-pad faint">End steps don't lead anywhere.</div>
  return (
    <div className="panel">
      <div className="panel-head">
        <h3>Paths out</h3>
        {!readOnly && <button className="btn sm" onClick={() => update((g) => { g.edges.push({ from: node.id, to: targets[0]?.id ?? node.id, ...(node.type === 'approval' ? { on: 'rejected' as const } : {}), ...(node.type === 'condition' ? { when: { field: 'days', op: 'gt', value: 3 } } : {}) }) })}><Plus /> Add path</button>}
      </div>
      <div className="ruled">
        {outs.length === 0 && <div className="panel-pad faint">No paths yet. Add one, or add a step after this one.</div>}
        {outs.map(({ e, i }) => (
          <div key={i} className="panel-pad stack" style={{ gap: 10 }}>
            <div className="row wrap">
              {node.type === 'approval' && (
                <select className="select sm" style={{ width: 'auto' }} disabled={readOnly} value={e.on ?? 'approved'} onChange={(ev) => setEdge(i, (x) => { x.on = ev.target.value as 'approved' | 'rejected' })}>
                  <option value="approved">When approved</option>
                  <option value="rejected">When declined</option>
                </select>
              )}
              {node.type === 'condition' && (
                <label className="check" style={{ fontSize: 13 }}><input type="checkbox" disabled={readOnly} checked={!!e.default} onChange={(ev) => setEdge(i, (x) => { x.default = ev.target.checked; if (x.default) delete x.when; else x.when = { field: 'days', op: 'gt', value: 3 } })} /> Default path</label>
              )}
              <span className="faint">go to</span>
              <select className="select sm" style={{ width: 'auto' }} disabled={readOnly} value={e.to} onChange={(ev) => setEdge(i, (x) => { x.to = ev.target.value })}>
                {targets.map((t) => <option key={t.id} value={t.id}>{t.name ?? t.id}</option>)}
              </select>
              <button className="link" style={{ fontSize: 12.5 }} onClick={() => onSelect(e.to)}>open</button>
              {!readOnly && <button className="btn ghost icon sm" style={{ marginLeft: 'auto' }} aria-label="Remove path" onClick={() => update((g) => { g.edges.splice(i, 1) })}><X /></button>}
            </div>
            {(node.type === 'condition' || node.type === 'parallel') && !e.default && (
              <CondEditor cond={e.when} readOnly={readOnly} optional={node.type === 'parallel'} onChange={(c) => setEdge(i, (x) => { if (c) x.when = c; else delete x.when })} />
            )}
            {(node.type === 'condition' || node.type === 'parallel') && (
              <input className="input sm" placeholder="Label shown on the diagram" disabled={readOnly} value={e.label ?? ''} onChange={(ev) => setEdge(i, (x) => { x.label = ev.target.value || undefined })} />
            )}
          </div>
        ))}
      </div>
    </div>
  )
}

type Cmp = { field: string; op: string; value?: unknown }

function CondEditor({ cond, onChange, readOnly, optional }: { cond?: Cond; onChange: (c: Cond | undefined) => void; readOnly: boolean; optional?: boolean }) {
  const combinator: 'all' | 'any' = cond && 'any' in cond ? 'any' : 'all'
  const rules: Cmp[] = !cond ? [] : 'all' in cond ? (cond.all as Cmp[]) : 'any' in cond ? (cond.any as Cmp[]) : 'field' in cond ? [cond] : []
  const emit = (rs: Cmp[], comb = combinator) => onChange(rs.length === 0 ? undefined : rs.length === 1 ? rs[0] : comb === 'all' ? { all: rs } : { any: rs })
  const setRule = (i: number, patch: Partial<Cmp>) => emit(rules.map((r, j) => (j === i ? { ...r, ...patch } : r)))
  return (
    <div className="stack-sm" style={{ background: 'var(--surface-2)', padding: 12, borderRadius: 4 }}>
      <datalist id="field-hints">{FIELD_HINTS.map((f) => <option key={f} value={f} />)}</datalist>
      {rules.length > 1 && (
        <div className="row" style={{ fontSize: 13 }}>
          Match
          <select className="select sm" style={{ width: 'auto' }} disabled={readOnly} value={combinator} onChange={(e) => emit(rules, e.target.value as 'all' | 'any')}>
            <option value="all">all of these</option><option value="any">any of these</option>
          </select>
        </div>
      )}
      {rules.length === 0 && <span className="faint" style={{ fontSize: 13 }}>{optional ? 'Always taken.' : 'No rule yet.'}</span>}
      {rules.map((r, i) => (
        <div key={i} className="row wrap" style={{ gap: 6 }}>
          <input className="input sm mono" list="field-hints" style={{ width: 150 }} disabled={readOnly} value={r.field} onChange={(e) => setRule(i, { field: e.target.value })} aria-label="Field" />
          <select className="select sm" style={{ width: 'auto' }} disabled={readOnly} value={r.op} onChange={(e) => setRule(i, { op: e.target.value })} aria-label="Comparison">{OPS.map(([k, l]) => <option key={k} value={k}>{l}</option>)}</select>
          {!['exists', 'empty'].includes(r.op) && (
            <input className="input sm" style={{ width: 150 }} disabled={readOnly} aria-label="Value"
              value={Array.isArray(r.value) ? r.value.join(', ') : String(r.value ?? '')}
              onChange={(e) => { const raw = e.target.value; setRule(i, { value: r.op === 'in' ? raw.split(',').map((s) => s.trim()).filter(Boolean) : raw !== '' && !isNaN(Number(raw)) ? Number(raw) : raw }) }} />
          )}
          {!readOnly && <button className="btn ghost icon sm" aria-label="Remove rule" onClick={() => emit(rules.filter((_, j) => j !== i))}><X /></button>}
        </div>
      ))}
      {!readOnly && <div><button className="link" style={{ fontSize: 12.5 }} onClick={() => emit([...rules, { field: 'days', op: 'gt', value: 3 }])}>+ Add rule</button></div>}
    </div>
  )
}

function Simulator({ graph, onClose }: { graph: Graph; onClose: () => void }) {
  const { me } = useSession()
  const { data: units } = useUnits()
  const [unit, setUnit] = useState('')
  const [rows, setRows] = useState<[string, string][]>([['days', '5'], ['type', 'Medical']])
  const sim = useMutation({
    mutationFn: () => {
      const data = Object.fromEntries(rows.filter(([k]) => k).map(([k, v]) => [k, v !== '' && !isNaN(Number(v)) ? Number(v) : v]))
      return api.post<{ trace: TraceStep[]; errors: string[] }>('/workflows/simulate', { graph, data, org_unit_id: unit || units?.[units.length - 1]?.id || me.grants[0]?.org_unit_id })
    },
  })
  return (
    <div className="panel" style={{ marginTop: 16 }}>
      <div className="panel-head"><h3>Test run</h3><button className="btn ghost icon sm" onClick={onClose} aria-label="Close"><X /></button></div>
      <div className="panel-pad grid-2" style={{ alignItems: 'start' }}>
        <div className="stack">
          <p className="muted" style={{ margin: 0, fontSize: 13.5 }}>See the route a request would take and exactly who would be asked — nothing is saved.</p>
          <Field label="Raised in"><select className="select" value={unit} onChange={(e) => setUnit(e.target.value)}><option value="">Pick a group…</option>{units?.map((u) => <option key={u.id} value={u.id}>{unitLabel(u)}</option>)}</select></Field>
          <div className="label">Request details</div>
          {rows.map(([k, v], i) => (
            <div key={i} className="row">
              <input className="input sm mono" value={k} onChange={(e) => setRows(rows.map((r, j) => (j === i ? [e.target.value, r[1]] : r)))} aria-label="Field" />
              <input className="input sm" value={v} onChange={(e) => setRows(rows.map((r, j) => (j === i ? [r[0], e.target.value] : r)))} aria-label="Value" />
              <button className="btn ghost icon sm" aria-label="Remove" onClick={() => setRows(rows.filter((_, j) => j !== i))}><X /></button>
            </div>
          ))}
          <div className="row"><button className="link" style={{ fontSize: 12.5 }} onClick={() => setRows([...rows, ['', '']])}>+ Add detail</button><span className="spacer" /><button className="btn primary sm" onClick={() => sim.mutate()} disabled={sim.isPending}><FlaskConical /> Run</button></div>
        </div>
        <div>
          {sim.error && <Notice tone="bad">{errorText(sim.error)}</Notice>}
          {sim.data && (
            <div className="stack-sm">
              {sim.data.errors.length > 0 && <Notice tone="warn">{sim.data.errors.length} issue{sim.data.errors.length === 1 ? '' : 's'} would block publishing.</Notice>}
              <ol className="timeline" style={{ margin: 0, padding: 0, listStyle: 'none' }}>
                {sim.data.trace.map((t, i) => (
                  <li key={i} className="tl-item">
                    <div className={`tl-dot ${t.type === 'approval' ? 'active' : t.outcome === 'rejected' ? 'bad' : t.type === 'end' ? 'ok' : ''}`} />
                    <div>
                      <div style={{ fontWeight: 500, fontSize: 13.5 }}>{t.name}</div>
                      {t.type === 'approval' && <div className="muted" style={{ fontSize: 13 }}>{t.approvers?.length ? t.approvers.join(', ') : 'Nobody found — the fallback applies'}</div>}
                      {t.chose && <div className="muted" style={{ fontSize: 13 }}>→ {t.chose}</div>}
                      {t.type === 'end' && <div className="muted" style={{ fontSize: 13 }}>{t.outcome === 'rejected' ? 'Declined' : 'Approved'}</div>}
                    </div>
                  </li>
                ))}
              </ol>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}
