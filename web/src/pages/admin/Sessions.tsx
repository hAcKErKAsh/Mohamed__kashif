import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api } from '../../lib/api'
import { Avatar, Empty, PageHead, Spinner, ago, useToast } from '../../components/ui'

interface S { id: string; user_id: string; full_name: string; email: string; ip: string | null; user_agent: string | null; created_at: string; last_seen_at: string; expires_at: string }

export default function Sessions() {
  const qc = useQueryClient()
  const toast = useToast()
  const { data } = useQuery({ queryKey: ['admin-sessions'], queryFn: () => api.get<S[]>('/admin/sessions'), refetchInterval: 30_000 })
  const revoke = useMutation({ mutationFn: (id: string) => api.del(`/admin/sessions/${id}`), onSuccess: () => { toast('Session ended.'); qc.invalidateQueries({ queryKey: ['admin-sessions'] }) } })
  return (
    <main className="content">
      <PageHead eyebrow="Control room" title="Live sessions">Everyone currently signed in. Ending a session signs that device out immediately.</PageHead>
      <div className="panel table-wrap">
        {!data ? <Spinner /> : data.length === 0 ? <Empty title="No one is signed in" /> : (
          <table className="table">
            <thead><tr><th>Person</th><th>Device</th><th>IP</th><th>Signed in</th><th>Last active</th><th /></tr></thead>
            <tbody>
              {data.map((s) => (
                <tr key={s.id}>
                  <td><div className="row"><Avatar name={s.full_name} size="sm" /><div><div style={{ fontWeight: 500 }}>{s.full_name}</div><div className="faint" style={{ fontSize: 12 }}>{s.email}</div></div></div></td>
                  <td className="faint" style={{ maxWidth: 260, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }} title={s.user_agent ?? ''}>{s.user_agent || '—'}</td>
                  <td className="mono faint">{s.ip ?? '—'}</td>
                  <td className="faint">{ago(s.created_at)}</td>
                  <td className="faint">{ago(s.last_seen_at)}</td>
                  <td className="right"><button className="btn sm danger" onClick={() => revoke.mutate(s.id)}>End</button></td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </main>
  )
}
