import { Fragment, useState } from 'react'
import { useInfiniteQuery, useQuery, useQueryClient } from '@tanstack/react-query'
import { ShieldCheck } from 'lucide-react'
import { api, qs } from '../../lib/api'
import { Notice, PageHead, Spinner, fmtDate } from '../../components/ui'

interface Row { seq: number; at: string; action: string; entity_type: string; entity_id: string | null; ip: string | null; detail: Record<string, unknown>; actor_name: string | null; hash: string }

export default function Audit() {
  const [action, setAction] = useState('')
  const [open, setOpen] = useState<number | null>(null)
  const qc = useQueryClient()
  const q = useInfiniteQuery({
    queryKey: ['audit', action],
    queryFn: ({ pageParam }) => api.get<Row[]>(`/admin/audit${qs({ before: pageParam, action })}`),
    initialPageParam: undefined as number | undefined,
    getNextPageParam: (last) => (last.length === 100 ? last[last.length - 1].seq : undefined),
  })
  const verify = useQuery({ queryKey: ['audit-verify'], queryFn: () => api.get<{ rows: number; intact: boolean; first_broken_seq: number | null }>('/admin/audit/verify'), enabled: false })
  const rows = q.data?.pages.flat()
  return (
    <main className="content wide" style={{ maxWidth: 1280 }}>
      <PageHead eyebrow="Control room" title="Audit trail" actions={<button className="btn" onClick={() => { verify.refetch(); qc.invalidateQueries({ queryKey: ['audit'] }) }}><ShieldCheck /> Verify integrity</button>}>
        Every sign-in, permission change and decision. Entries can't be edited or deleted, and each one is chained to the last by SHA-256.
      </PageHead>
      {verify.data && (
        <div style={{ marginBottom: 16 }}>
          {verify.data.intact ? <Notice tone="ok">Verified {verify.data.rows.toLocaleString()} entries. The chain is unbroken.</Notice> : <Notice tone="bad">Chain broken at entry #{verify.data.first_broken_seq}.</Notice>}
        </div>
      )}
      <div className="row wrap" style={{ marginBottom: 14 }}>
        <select className="select" style={{ width: 'auto' }} value={action} onChange={(e) => setAction(e.target.value)} aria-label="Filter by event type">
          <option value="">All events</option>
          <option value="auth.">Sign-in & sessions</option>
          <option value="user.">Account changes</option>
          <option value="membership.">Role grants</option>
          <option value="role.">Role definitions</option>
          <option value="workflow.">Workflows</option>
          <option value="approval.">Approval decisions</option>
          <option value="settings.">Settings</option>
          <option value="content.">Content</option>
          <option value="post.">Community</option>
        </select>
      </div>
      <div className="panel table-wrap">
        {!rows ? <Spinner /> : (
          <table className="table">
            <thead><tr><th>#</th><th>When</th><th>Event</th><th>By</th><th>Subject</th><th>From</th><th>Hash</th></tr></thead>
            <tbody>
              {rows.map((r) => (
                <Fragment key={r.seq}>
                  <tr className="clickable" onClick={() => setOpen(open === r.seq ? null : r.seq)}>
                    <td className="mono faint num">{r.seq}</td>
                    <td className="faint" style={{ whiteSpace: 'nowrap' }}>{fmtDate(r.at, true)}</td>
                    <td><span className="code">{r.action}</span></td>
                    <td>{r.actor_name ?? <span className="faint">—</span>}</td>
                    <td className="faint">{r.entity_type}{r.entity_id ? ` · ${r.entity_id.slice(0, 8)}` : ''}</td>
                    <td className="mono faint">{r.ip ?? '—'}</td>
                    <td className="mono faint" title={r.hash}>{r.hash.slice(0, 10)}…</td>
                  </tr>
                  {open === r.seq && (
                    <tr><td colSpan={7} style={{ background: 'var(--surface-2)' }}><pre className="mono" style={{ margin: 0, whiteSpace: 'pre-wrap', fontSize: 12 }}>{JSON.stringify(r.detail, null, 2)}</pre></td></tr>
                  )}
                </Fragment>
              ))}
            </tbody>
          </table>
        )}
      </div>
      {q.hasNextPage && <button className="btn" style={{ marginTop: 14 }} onClick={() => q.fetchNextPage()}>Load older</button>}
    </main>
  )
}
