import { useMemo, useState } from 'react'
import { Link, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowLeft, Check, CornerUpLeft, GitMerge, Send, X } from 'lucide-react'
import { api } from '../lib/api'
import type { FieldDef, Graph } from '../lib/types'
import { FlowDiagram, NodeState } from '../components/FlowDiagram'
import { DataTable, FormRenderer } from '../components/FormRenderer'
import { Avatar, Notice, Spinner, StatusTag, ago, errorText, fmtDate, useToast } from '../components/ui'

interface Task { id: string; assignee: string; assignee_id: string; status: string; comment: string | null; decided_at: string | null }
interface Step { id: string; node_id: string; name: string; type: string; status: string; mode: string; quorum: number | null; round: number; activated_at: string; completed_at: string | null; due_at: string | null; escalated: boolean; tasks: Task[] }
interface Event { id: number; at: string; kind: string; node_id: string | null; message: string | null; actor: string | null; detail: Record<string, unknown> }
interface Detail {
  request: { id: string; title: string; status: string; workflow: string; unit: string; requester: string; created_at: string; completed_at: string | null; round: number; data: Record<string, unknown>; subject_type: string }
  steps: Step[]
  events: Event[]
  graph: Graph
  form_fields: FieldDef[] | null
  my_task_id: string | null
  is_requester: boolean
}

const EVENT_TEXT: Record<string, string> = {
  submitted: 'submitted the request',
  resubmitted: 'resubmitted with changes',
  approved: 'approved',
  rejected: 'declined',
  returned: 'sent it back for changes',
  cancelled: 'cancelled the request',
  stage_started: 'Stage opened',
  routed: 'Routed',
  joined: 'All parallel approvals complete',
  escalated: 'Escalated after the time limit',
  skipped: 'Stage skipped',
}

export default function RequestDetail() {
  const { id } = useParams()
  const qc = useQueryClient()
  const toast = useToast()
  const { data, error } = useQuery({ queryKey: ['request', id], queryFn: () => api.get<Detail>(`/approvals/${id}`) })
  const [comment, setComment] = useState('')
  const [editing, setEditing] = useState<Record<string, unknown> | null>(null)

  const invalidate = () => {
    qc.invalidateQueries({ queryKey: ['request', id] })
    qc.invalidateQueries({ queryKey: ['inbox'] })
    qc.invalidateQueries({ queryKey: ['my-requests'] })
    qc.invalidateQueries({ queryKey: ['dashboard'] })
  }
  const decide = useMutation({
    mutationFn: (decision: 'approve' | 'reject' | 'return') => api.post(`/approvals/tasks/${data!.my_task_id}/decide`, { decision, comment }),
    onSuccess: (_, d) => {
      toast(d === 'approve' ? 'Approved.' : d === 'reject' ? 'Declined. The requester has been told why.' : 'Sent back to the requester.')
      setComment('')
      invalidate()
    },
    onError: (e) => toast(errorText(e), 'bad'),
  })
  const cancel = useMutation({ mutationFn: () => api.post(`/approvals/${id}/cancel`), onSuccess: () => { toast('Request cancelled.'); invalidate() }, onError: (e) => toast(errorText(e), 'bad') })
  const resubmit = useMutation({
    mutationFn: () => api.post(`/approvals/${id}/resubmit`, { data: editing }),
    onSuccess: () => { toast('Resubmitted. It starts again from the first stage.'); setEditing(null); invalidate() },
    onError: (e) => toast(errorText(e), 'bad'),
  })

  const { states, taken } = useMemo(() => {
    const states: Record<string, NodeState> = {}
    const taken = new Set<string>()
    if (!data) return { states, taken }
    const round = data.request.round
    for (const s of data.steps.filter((s) => s.round === round)) {
      states[s.node_id] = s.status === 'active' || s.status === 'waiting' ? 'active' : s.status === 'approved' || s.status === 'passed' ? 'done' : s.status === 'rejected' || s.status === 'returned' ? 'bad' : 'skip'
    }
    const start = data.graph.nodes.find((n) => n.type === 'start')
    if (start) states[start.id] = 'done'
    if (data.request.status === 'approved' || data.request.status === 'rejected') {
      // Mark the end node that was reached.
      const outcome = data.request.status
      const ends = data.graph.nodes.filter((n) => n.type === 'end' && n.outcome === outcome)
      const reached = ends.find((n) => data.graph.edges.some((e) => e.to === n.id && states[e.from] && states[e.from] !== 'skip'))
      if (reached) states[reached.id] = outcome === 'approved' ? 'done' : 'bad'
    }
    for (const e of data.graph.edges) {
      const a = states[e.from], b = states[e.to]
      const outcomeOk = !e.on || (e.on === 'rejected' ? a === 'bad' : a === 'done')
      if (a && b && b !== 'skip' && outcomeOk && (a === 'done' || a === 'bad')) taken.add(`${e.from}>${e.to}`)
    }
    return { states, taken }
  }, [data])

  if (error) return <main className="content"><Notice tone="bad">{errorText(error)}</Notice></main>
  if (!data) return <main className="content"><Spinner /></main>
  const r = data.request
  const roundSteps = data.steps.filter((s) => s.round === r.round && s.type === 'approval')
  const decisionNeedsReason = !comment.trim()

  return (
    <main className="content">
      <Link to="/approvals" className="row faint" style={{ gap: 6, fontSize: 13, marginBottom: 14 }}><ArrowLeft size={14} /> Approvals</Link>
      <header className="page-head">
        <div>
          <div className="eyebrow">{r.workflow} · {r.unit}</div>
          <h1>{r.title}</h1>
          <p className="row wrap" style={{ gap: 10 }}>
            <StatusTag status={r.status} />
            <span>Raised by {r.requester} {ago(r.created_at)}</span>
            {r.round > 1 && <span className="faint">· round {r.round}</span>}
            {r.completed_at && <span className="faint">· closed {fmtDate(r.completed_at, true)}</span>}
          </p>
        </div>
        {data.is_requester && (r.status === 'in_progress' || r.status === 'returned') && (
          <div className="actions">
            {r.status === 'returned' && !editing && (
              <button className="btn primary" onClick={() => setEditing({ ...r.data })}><CornerUpLeft /> Edit and resubmit</button>
            )}
            <button className="btn danger" disabled={cancel.isPending} onClick={() => confirm('Cancel this request? Approvers will be told it is no longer needed.') && cancel.mutate()}>Cancel request</button>
          </div>
        )}
      </header>

      {r.status === 'returned' && data.is_requester && (
        <div style={{ marginBottom: 20 }}>
          <Notice tone="warn">
            This was sent back to you: “{[...data.events].reverse().find((e) => e.kind === 'returned')?.message ?? 'Please review and resubmit.'}”
          </Notice>
        </div>
      )}

      <section style={{ marginBottom: 28 }}>
        <div className="section-title"><h2>Route</h2><span className="faint" style={{ fontSize: 12.5 }}>The highlighted path shows where it has been</span></div>
        <FlowDiagram graph={data.graph} states={states} taken={taken} />
      </section>

      <div className="split">
        <div className="stack" style={{ gap: 24 }}>
          {data.my_task_id && r.status === 'in_progress' && (
            <section className="panel" style={{ borderColor: 'var(--accent)' }}>
              <div className="panel-head"><h3>Your decision</h3><span className="faint" style={{ fontSize: 12.5 }}>Recorded permanently in the audit trail</span></div>
              <div className="panel-pad stack">
                <textarea className="textarea" placeholder="Add a note for the requester (required to decline or send back)" value={comment} onChange={(e) => setComment(e.target.value)} />
                <div className="row wrap" style={{ gap: 8 }}>
                  <button className="btn primary" disabled={decide.isPending} onClick={() => decide.mutate('approve')}><Check /> Approve</button>
                  <button className="btn" disabled={decide.isPending || decisionNeedsReason} title={decisionNeedsReason ? 'Add a note first' : ''} onClick={() => decide.mutate('return')}><CornerUpLeft /> Send back for changes</button>
                  <button className="btn danger" disabled={decide.isPending || decisionNeedsReason} title={decisionNeedsReason ? 'Add a note first' : ''} onClick={() => decide.mutate('reject')}><X /> Decline</button>
                </div>
              </div>
            </section>
          )}

          <section className="panel">
            <div className="panel-head"><h3>{editing ? 'Edit your request' : 'Details'}</h3></div>
            <div className="panel-pad">
              {editing && data.form_fields ? (
                <div className="stack">
                  <FormRenderer fields={data.form_fields} value={editing} onChange={setEditing} />
                  <div className="row" style={{ justifyContent: 'flex-end' }}>
                    <button className="btn ghost" onClick={() => setEditing(null)}>Discard</button>
                    <button className="btn primary" disabled={resubmit.isPending} onClick={() => resubmit.mutate()}><Send /> Resubmit</button>
                  </div>
                </div>
              ) : editing ? (
                <div className="stack">
                  <p className="muted" style={{ margin: 0 }}>Resubmit to send it through the workflow again.</p>
                  <div className="row" style={{ justifyContent: 'flex-end' }}>
                    <button className="btn ghost" onClick={() => setEditing(null)}>Discard</button>
                    <button className="btn primary" disabled={resubmit.isPending} onClick={() => resubmit.mutate()}><Send /> Resubmit</button>
                  </div>
                </div>
              ) : (
                <DataTable data={r.data} fields={data.form_fields} />
              )}
            </div>
          </section>

          <section className="panel">
            <div className="panel-head"><h3>Stages</h3></div>
            <div className="ruled">
              {roundSteps.length === 0 && <div className="panel-pad faint">No approval stages have opened yet.</div>}
              {roundSteps.map((s) => (
                <div key={s.id} className="panel-pad">
                  <div className="row between">
                    <div>
                      <div style={{ fontWeight: 500 }}>{s.name}</div>
                      <div className="faint" style={{ fontSize: 12.5 }}>
                        {s.mode === 'all' ? 'Everyone must agree' : s.mode === 'quorum' ? `${s.quorum} must agree` : 'Any one approver'}
                        {s.due_at && ` · due ${fmtDate(s.due_at, true)}`}
                        {s.escalated && ' · escalated'}
                      </div>
                    </div>
                    <StatusTag status={s.status} />
                  </div>
                  <div className="stack-sm" style={{ marginTop: 12 }}>
                    {s.tasks.map((t) => (
                      <div key={t.id} className="row" style={{ gap: 10 }}>
                        <Avatar name={t.assignee} size="sm" />
                        <span style={{ fontSize: 13.5 }}>{t.assignee}</span>
                        <StatusTag status={t.status} />
                        {t.comment && <span className="muted" style={{ fontSize: 13, minWidth: 0 }}>“{t.comment}”</span>}
                      </div>
                    ))}
                  </div>
                </div>
              ))}
            </div>
          </section>
        </div>

        <aside>
          <div className="section-title"><h2>History</h2></div>
          <div className="panel panel-pad">
            <div className="timeline">
              {data.events.map((e) => {
                const tone = e.kind === 'approved' ? 'ok' : e.kind === 'rejected' ? 'bad' : e.kind === 'returned' || e.kind === 'escalated' ? 'warn' : e.kind === 'stage_started' ? 'active' : ''
                const node = data.graph.nodes.find((n) => n.id === e.node_id)
                const text = e.actor ? `${e.actor} ${EVENT_TEXT[e.kind] ?? e.kind}` : `${EVENT_TEXT[e.kind] ?? e.kind}${node ? `: ${node.name ?? node.id}` : ''}`
                return (
                  <div className="tl-item" key={e.id}>
                    <div className={`tl-dot ${tone}`}>
                      {e.kind === 'approved' ? <Check /> : e.kind === 'rejected' ? <X /> : e.kind === 'joined' || e.kind === 'routed' ? <GitMerge style={{ color: 'var(--ink-3)' }} /> : null}
                    </div>
                    <div>
                      <div style={{ fontSize: 13.5 }}>
                        {text}
                        {e.kind === 'routed' && typeof e.detail.path === 'string' && <span className="muted"> → {e.detail.path}</span>}
                        {e.kind === 'stage_started' && typeof e.detail.approvers === 'number' && <span className="faint"> · {e.detail.approvers} approver{e.detail.approvers === 1 ? '' : 's'}</span>}
                      </div>
                      {e.message && <div className="muted" style={{ fontSize: 13, marginTop: 2 }}>“{e.message}”</div>}
                      <div className="faint" style={{ fontSize: 12 }}>{fmtDate(e.at, true)}</div>
                    </div>
                  </div>
                )
              })}
            </div>
          </div>
        </aside>
      </div>
    </main>
  )
}
