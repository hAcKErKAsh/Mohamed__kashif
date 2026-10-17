import { useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useMutation, useQuery } from '@tanstack/react-query'
import { GitBranch, Plus } from 'lucide-react'
import { api } from '../../lib/api'
import type { Graph } from '../../lib/types'
import { Empty, Field, Modal, Notice, PageHead, Spinner, StatusTag, errorText, fmtDate } from '../../components/ui'

interface Version { id: string; version: number; status: string; name: string; description: string; created_at: string; published_at: string | null; stages: number }
interface WorkflowGroup { key: string; versions: Version[]; in_flight: number }

const TEMPLATES: Record<string, { label: string; graph: Graph }> = {
  single: {
    label: 'One approver',
    graph: {
      nodes: [
        { id: 'start', type: 'start', name: 'Submitted' },
        { id: 'review', type: 'approval', name: 'Review', approvers: { kind: 'role', role: 'teacher', search: 'nearest' }, mode: 'any' },
        { id: 'approved', type: 'end', name: 'Approved', outcome: 'approved' },
      ],
      edges: [{ from: 'start', to: 'review' }, { from: 'review', to: 'approved', on: 'approved' }],
    },
  },
  escalating: {
    label: 'Two levels, second only when needed',
    graph: {
      nodes: [
        { id: 'start', type: 'start', name: 'Submitted' },
        { id: 'first', type: 'approval', name: 'First review', approvers: { kind: 'role', role: 'teacher', search: 'nearest' } },
        { id: 'check', type: 'condition', name: 'Needs more?' },
        { id: 'second', type: 'approval', name: 'Senior review', approvers: { kind: 'role', role: 'headmaster', search: 'nearest' } },
        { id: 'approved', type: 'end', name: 'Approved', outcome: 'approved' },
      ],
      edges: [
        { from: 'start', to: 'first' },
        { from: 'first', to: 'check', on: 'approved' },
        { from: 'check', to: 'second', when: { field: 'amount', op: 'gt', value: 100 }, label: 'Over 100' },
        { from: 'check', to: 'approved', default: true, label: 'Otherwise' },
        { from: 'second', to: 'approved', on: 'approved' },
      ],
    },
  },
  parallel: {
    label: 'Two people in parallel',
    graph: {
      nodes: [
        { id: 'start', type: 'start', name: 'Submitted' },
        { id: 'split', type: 'parallel', name: 'Ask both' },
        { id: 'a', type: 'approval', name: 'Guardian', approvers: { kind: 'guardians' } },
        { id: 'b', type: 'approval', name: 'Teacher', approvers: { kind: 'role', role: 'teacher', search: 'nearest' } },
        { id: 'join', type: 'join', name: 'Both agreed' },
        { id: 'approved', type: 'end', name: 'Approved', outcome: 'approved' },
      ],
      edges: [
        { from: 'start', to: 'split' }, { from: 'split', to: 'a' }, { from: 'split', to: 'b' },
        { from: 'a', to: 'join', on: 'approved' }, { from: 'b', to: 'join', on: 'approved' }, { from: 'join', to: 'approved' },
      ],
    },
  },
}

export default function Workflows() {
  const nav = useNavigate()
  const { data } = useQuery({ queryKey: ['workflows'], queryFn: () => api.get<WorkflowGroup[]>('/workflows') })
  const [creating, setCreating] = useState(false)
  return (
    <main className="content">
      <PageHead eyebrow="Control room" title="Workflows" actions={<button className="btn primary" onClick={() => setCreating(true)}><Plus /> New workflow</button>}>
        Design how requests are approved, without code. Every published version is kept, and requests already under way finish on the version they started with.
      </PageHead>
      {!data && <Spinner />}
      {data?.length === 0 && <div className="panel"><Empty title="No workflows yet" /></div>}
      <div className="grid-2">
        {data?.map((w) => {
          const live = w.versions.find((v) => v.status === 'published')
          const latest = w.versions[0]
          return (
            <button key={w.key} className="panel panel-pad stack" style={{ textAlign: 'left', cursor: 'pointer', gap: 10 }} onClick={() => nav(`/admin/workflows/${latest.id}`)}>
              <div className="row between">
                <span className="row" style={{ gap: 8 }}><GitBranch size={15} style={{ color: 'var(--accent)' }} /><span className="mono faint" style={{ fontSize: 12 }}>{w.key}</span></span>
                {live ? <StatusTag status="published" /> : <StatusTag status="draft" />}
              </div>
              <div className="serif" style={{ fontSize: 21, lineHeight: 1.2 }}>{(live ?? latest).name}</div>
              <div className="muted" style={{ fontSize: 13.5 }}>{(live ?? latest).description}</div>
              <div className="row faint wrap" style={{ fontSize: 12.5, gap: 14 }}>
                <span>{(live ?? latest).stages} approval stage{(live ?? latest).stages === 1 ? '' : 's'}</span>
                <span>v{live?.version ?? latest.version}{latest.status === 'draft' && live ? ` · draft v${latest.version} in progress` : ''}</span>
                <span>{w.in_flight} in flight</span>
                {live?.published_at && <span>published {fmtDate(live.published_at)}</span>}
              </div>
            </button>
          )
        })}
      </div>
      {creating && <NewWorkflow onClose={() => setCreating(false)} />}
    </main>
  )
}

function NewWorkflow({ onClose }: { onClose: () => void }) {
  const nav = useNavigate()
  const [f, setF] = useState({ name: '', key: '', template: 'single' })
  const [err, setErr] = useState('')
  const m = useMutation({
    mutationFn: () => api.post<{ id: string }>('/workflows', { key: f.key, name: f.name, description: '', graph: TEMPLATES[f.template].graph }),
    onSuccess: (r) => nav(`/admin/workflows/${r.id}`),
    onError: (e) => setErr(errorText(e)),
  })
  return (
    <Modal title="New workflow" onClose={onClose} footer={<><button className="btn ghost" onClick={onClose}>Cancel</button><button className="btn primary" disabled={!f.name || !f.key || m.isPending} onClick={() => m.mutate()}>Create draft</button></>}>
      <div className="stack">
        {err && <Notice tone="bad">{err}</Notice>}
        <Field label="Name" required><input className="input" value={f.name} onChange={(e) => setF({ ...f, name: e.target.value, key: e.target.value.toLowerCase().replace(/[^a-z0-9]+/g, '_').replace(/^_|_$/g, '').slice(0, 48) })} placeholder="e.g. Library purchase" /></Field>
        <Field label="Key" hint="Forms and policies refer to the workflow by this key."><input className="input mono" value={f.key} onChange={(e) => setF({ ...f, key: e.target.value })} /></Field>
        <Field label="Start from">
          <div className="stack-sm">
            {Object.entries(TEMPLATES).map(([k, t]) => (
              <label key={k} className="check"><input type="radio" name="tpl" checked={f.template === k} onChange={() => setF({ ...f, template: k })} /> {t.label}</label>
            ))}
          </div>
        </Field>
      </div>
    </Modal>
  )
}
