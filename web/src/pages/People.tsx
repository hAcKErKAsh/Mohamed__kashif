import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { Search } from 'lucide-react'
import { api, qs } from '../lib/api'
import { Avatar, Empty, PageHead, Spinner } from '../components/ui'

interface Person { id: string; full_name: string; email: string; roles: { role: string; color: string; unit: string }[] }

export default function People() {
  const [q, setQ] = useState('')
  const { data } = useQuery({ queryKey: ['people', q], queryFn: () => api.get<Person[]>(`/people${qs({ q })}`), placeholderData: (p) => p })
  return (
    <main className="content">
      <PageHead eyebrow="Directory" title="People">Everyone in the groups you work with.</PageHead>
      <div className="search" style={{ maxWidth: 360, marginBottom: 18 }}>
        <Search />
        <input className="input" placeholder="Search by name" value={q} onChange={(e) => setQ(e.target.value)} aria-label="Search people" />
      </div>
      {!data && <Spinner />}
      {data?.length === 0 && <div className="panel"><Empty title="No one matches" /></div>}
      <div className="grid-3">
        {data?.map((p) => (
          <div key={p.id} className="panel panel-pad row" style={{ alignItems: 'flex-start', gap: 12 }}>
            <Avatar name={p.full_name} />
            <div style={{ minWidth: 0 }}>
              <div style={{ fontWeight: 500 }}>{p.full_name}</div>
              <div className="faint" style={{ fontSize: 12.5, overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.email}</div>
              <div className="stack-sm" style={{ marginTop: 8, gap: 4 }}>
                {p.roles.map((r, i) => (
                  <span key={i} className="role-chip"><i style={{ background: r.color }} />{r.role} · {r.unit}</span>
                ))}
              </div>
            </div>
          </div>
        ))}
      </div>
    </main>
  )
}
