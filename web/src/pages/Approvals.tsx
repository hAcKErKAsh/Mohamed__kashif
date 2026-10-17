import { useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Check, Clock } from 'lucide-react'
import { api, qs } from '../lib/api'
import { useSession } from '../lib/session'
import { Empty, PageHead, Spinner, StatusTag, ago, errorText, fmtDate, useToast } from '../components/ui'

interface InboxItem { task_id: string; request_id: string; title: string; stage: string; mode: string; requester: string; unit: string; workflow: string; due_at: string | null; assigned_at: string; data: Record<string, unknown> }
interface Decided { task_id: string; request_id: string; title: string; decision: string; decided_at: string; request_status: string; requester: string }
interface Req { id: string; title: string; status: string; workflow: string; unit: string; requester: string; created_at: string; updated_at: string; current: string | null; round: number }

export default function Approvals() {
  const { can } = useSession()
  const [params, setParams] = useSearchParams()
  const tab = params.get('tab') ?? 'inbox'
  const { data: inbox } = useQuery({ queryKey: ['inbox'], queryFn: () => api.get<{ pending: InboxItem[]; recent: Decided[] }>('/approvals/inbox') })
  const { data: mine } = useQuery({ queryKey: ['my-requests'], queryFn: () => api.get<Req[]>('/approvals/mine') })
  const tabs = [
    ['inbox', 'Waiting on me', inbox?.pending.length],
    ['mine', 'My requests', mine?.length],
    ['decided', 'Decided by me', inbox?.recent.length],
    ...(can('approvals.view_all') ? [['all', 'Everything in my scope', undefined]] : []),
  ] as [string, string, number | undefined][]

  return (
    <main className="content">
      <PageHead eyebrow="Approvals" title="Decisions and requests">
        Every request moves through its workflow stage by stage. Nothing is approved by the person who asked for it.
      </PageHead>
      <div className="tabs" role="tablist">
        {tabs.map(([k, label, n]) => (
          <button key={k} role="tab" aria-selected={tab === k} className={tab === k ? 'on' : ''} onClick={() => setParams({ tab: k })}>
            {label} {n !== undefined && <span className="n">{n}</span>}
          </button>
        ))}
      </div>
      {tab === 'inbox' && <Inbox items={inbox?.pending} />}
      {tab === 'mine' && <RequestTable rows={mine} empty="You haven't raised any requests yet." showRequester={false} />}
      {tab === 'decided' && <DecidedList rows={inbox?.recent} />}
      {tab === 'all' && <AllRequests />}
    </main>
  )
}

function Inbox({ items }: { items?: InboxItem[] }) {
  const qc = useQueryClient()
  const toast = useToast()
  const approve = useMutation({
    mutationFn: (task: string) => api.post(`/approvals/tasks/${task}/decide`, { decision: 'approve' }),
    onSuccess: () => {
      toast('Approved. The request has moved on.')
      qc.invalidateQueries({ queryKey: ['inbox'] })
      qc.invalidateQueries({ queryKey: ['dashboard'] })
    },
    onError: (e) => toast(errorText(e), 'bad'),
  })
  if (!items) return <Spinner />
  if (items.length === 0) return <div className="panel"><Empty title="Nothing waiting on you">When a request reaches a stage you approve, it lands here.</Empty></div>
  return (
    <div className="panel table-wrap">
      <table className="table">
        <thead>
          <tr><th>Request</th><th>Your stage</th><th>From</th><th>Due</th><th className="right">Action</th></tr>
        </thead>
        <tbody>
          {items.map((t) => {
            const overdue = t.due_at && new Date(t.due_at) < new Date()
            return (
              <tr key={t.task_id}>
                <td>
                  <Link to={`/approvals/${t.request_id}`} style={{ fontWeight: 500 }}>{t.title}</Link>
                  <div className="faint" style={{ fontSize: 12.5 }}>{t.workflow} · {t.unit}</div>
                </td>
                <td>{t.stage}<div className="faint" style={{ fontSize: 12.5 }}>{t.mode === 'all' ? 'Everyone must agree' : t.mode === 'quorum' ? 'Quorum vote' : 'Any one of the approvers'}</div></td>
                <td>{t.requester}<div className="faint" style={{ fontSize: 12.5 }}>{ago(t.assigned_at)}</div></td>
                <td>{t.due_at ? <span className={`tag ${overdue ? 'bad' : ''}`}><Clock size={12} /> {overdue ? 'Overdue' : fmtDate(t.due_at, true)}</span> : <span className="faint">—</span>}</td>
                <td className="right">
                  <div className="row" style={{ justifyContent: 'flex-end', gap: 6 }}>
                    <Link to={`/approvals/${t.request_id}`} className="btn sm">Review</Link>
                    <button className="btn sm primary" disabled={approve.isPending} onClick={() => approve.mutate(t.task_id)}><Check /> Approve</button>
                  </div>
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

function DecidedList({ rows }: { rows?: Decided[] }) {
  const nav = useNavigate()
  if (!rows) return <Spinner />
  if (rows.length === 0) return <div className="panel"><Empty title="No decisions yet" /></div>
  return (
    <div className="panel table-wrap">
      <table className="table">
        <thead><tr><th>Request</th><th>From</th><th>Your decision</th><th>Request now</th><th>When</th></tr></thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.task_id} className="clickable" onClick={() => nav(`/approvals/${r.request_id}`)}>
              <td style={{ fontWeight: 500 }}>{r.title}</td>
              <td>{r.requester}</td>
              <td><StatusTag status={r.decision} /></td>
              <td><StatusTag status={r.request_status} /></td>
              <td className="faint">{ago(r.decided_at)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function RequestTable({ rows, empty, showRequester = true }: { rows?: Req[]; empty: string; showRequester?: boolean }) {
  const nav = useNavigate()
  if (!rows) return <Spinner />
  if (rows.length === 0) return <div className="panel"><Empty title="Nothing here">{empty}</Empty></div>
  return (
    <div className="panel table-wrap">
      <table className="table">
        <thead>
          <tr><th>Request</th>{showRequester && <th>Requester</th>}<th>Status</th><th>Currently with</th><th>Raised</th></tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.id} className="clickable" onClick={() => nav(`/approvals/${r.id}`)}>
              <td>
                <div style={{ fontWeight: 500 }}>{r.title}</div>
                <div className="faint" style={{ fontSize: 12.5 }}>{r.workflow} · {r.unit}{r.round > 1 ? ` · round ${r.round}` : ''}</div>
              </td>
              {showRequester && <td>{r.requester}</td>}
              <td><StatusTag status={r.status} /></td>
              <td className="muted">{r.current ?? '—'}</td>
              <td className="faint">{fmtDate(r.created_at)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function AllRequests() {
  const [status, setStatus] = useState('')
  const { data } = useQuery({ queryKey: ['all-requests', status], queryFn: () => api.get<Req[]>(`/approvals/all${qs({ status })}`) })
  return (
    <div className="stack">
      <div className="segmented" role="group" aria-label="Filter by status">
        {[['', 'All'], ['in_progress', 'In review'], ['returned', 'Sent back'], ['approved', 'Approved'], ['rejected', 'Declined']].map(([k, l]) => (
          <button key={k} className={status === k ? 'on' : ''} onClick={() => setStatus(k)}>{l}</button>
        ))}
      </div>
      <RequestTable rows={data} empty="No requests match." />
    </div>
  )
}
