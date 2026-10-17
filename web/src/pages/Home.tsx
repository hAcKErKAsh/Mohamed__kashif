import { Link } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { ArrowRight, Clock, Pin } from 'lucide-react'
import { api } from '../lib/api'
import { useSession } from '../lib/session'
import { Empty, StatusTag, ago, fmtDate } from '../components/ui'

interface Dashboard {
  pending_tasks: number
  overdue_tasks: number
  open_requests: number
  returned_requests: number
  announcements: { id: string; title: string; summary: string; unit: string; author: string; published_at: string; pinned: boolean; kind: string }[]
  admin: null | { active_users: number; online_users: number; requests_in_flight: number; failed_logins_24h: number; locked_accounts: number; requests_by_day: { day: string; requests: number }[] }
}

interface InboxItem { task_id: string; request_id: string; title: string; stage: string; requester: string; unit: string; due_at: string | null; assigned_at: string }
interface MyRequest { id: string; title: string; status: string; current: string | null; updated_at: string }

function greeting() {
  const h = new Date().getHours()
  return h < 12 ? 'Good morning' : h < 17 ? 'Good afternoon' : 'Good evening'
}

export function Home() {
  const { me, can, module } = useSession()
  const { data } = useQuery({ queryKey: ['dashboard'], queryFn: () => api.get<Dashboard>('/dashboard') })
  const { data: inbox } = useQuery({ queryKey: ['inbox'], queryFn: () => api.get<{ pending: InboxItem[] }>('/approvals/inbox') })
  const { data: mine } = useQuery({ queryKey: ['my-requests'], queryFn: () => api.get<MyRequest[]>('/approvals/mine') })
  const first = me.user.full_name.replace(/^Dr\.\s*/, '').split(' ')[0]
  const today = new Date().toLocaleDateString('en-GB', { weekday: 'long', day: 'numeric', month: 'long' })

  return (
    <main className="content">
      <header className="page-head">
        <div>
          <div className="eyebrow">{today}</div>
          <h1>{greeting()}, {first}.</h1>
          <p>
            {data?.pending_tasks
              ? `${data.pending_tasks} ${data.pending_tasks === 1 ? 'decision is' : 'decisions are'} waiting for you${data.overdue_tasks ? `, ${data.overdue_tasks} past due` : ''}.`
              : 'Nothing is waiting on you right now.'}
            {data?.returned_requests ? ` ${data.returned_requests} of your requests ${data.returned_requests === 1 ? 'was' : 'were'} sent back for changes.` : ''}
          </p>
        </div>
        {can('forms.submit') && module('forms') && (
          <div className="actions">
            <Link to="/forms" className="btn primary">New request</Link>
          </div>
        )}
      </header>

      <div className="panel stats" style={{ marginBottom: 28 }}>
        <div className="stat"><span className="eyebrow">Waiting on you</span><span className="value">{data?.pending_tasks ?? '–'}</span><span className="sub">approval decisions</span></div>
        <div className="stat"><span className="eyebrow">Past due</span><span className="value" style={{ color: data?.overdue_tasks ? 'var(--bad)' : undefined }}>{data?.overdue_tasks ?? '–'}</span><span className="sub">beyond their time limit</span></div>
        <div className="stat"><span className="eyebrow">Your open requests</span><span className="value">{data?.open_requests ?? '–'}</span><span className="sub">in review or sent back</span></div>
        <div className="stat"><span className="eyebrow">Notices</span><span className="value">{data?.announcements.length ?? '–'}</span><span className="sub">recent for your groups</span></div>
      </div>

      {me.children.length > 0 && (
        <section style={{ marginBottom: 28 }}>
          <div className="section-title"><h2>Your children</h2></div>
          <div className="grid-3">
            {me.children.map((c) => (
              <div key={c.id} className="panel panel-pad">
                <div style={{ fontWeight: 500 }}>{c.full_name}</div>
                <div className="faint" style={{ fontSize: 13 }}>Requests that need your consent appear under Approvals.</div>
              </div>
            ))}
          </div>
        </section>
      )}

      <div className="split">
        <div className="stack" style={{ gap: 28 }}>
          <section>
            <div className="section-title">
              <h2>Waiting on you</h2>
              <Link to="/approvals" className="link" style={{ fontSize: 13 }}>Open inbox</Link>
            </div>
            <div className="panel ruled">
              {inbox?.pending.length === 0 && <Empty title="You're all caught up">New requests that need your decision will appear here.</Empty>}
              {inbox?.pending.slice(0, 5).map((t) => (
                <Link key={t.task_id} to={`/approvals/${t.request_id}`} className="row" style={{ padding: '14px 18px', gap: 14 }}>
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div style={{ fontWeight: 500 }}>{t.title}</div>
                    <div className="faint" style={{ fontSize: 13 }}>{t.stage} · {t.unit} · from {t.requester}</div>
                  </div>
                  {t.due_at && (
                    <span className={`tag ${new Date(t.due_at) < new Date() ? 'bad' : ''}`}>
                      <Clock size={12} /> {new Date(t.due_at) < new Date() ? 'Overdue' : `Due ${ago(t.due_at)}`}
                    </span>
                  )}
                  <ArrowRight size={15} className="faint" />
                </Link>
              ))}
            </div>
          </section>

          {module('content') && can('content.view') && (
            <section>
              <div className="section-title">
                <h2>Notices</h2>
                <Link to="/notices" className="link" style={{ fontSize: 13 }}>All notices</Link>
              </div>
              <div className="panel ruled">
                {data?.announcements.length === 0 && <Empty title="No notices yet" />}
                {data?.announcements.map((n) => (
                  <Link key={n.id} to={`/notices/${n.id}`} style={{ display: 'block', padding: '16px 18px' }}>
                    <div className="row" style={{ gap: 8, marginBottom: 4 }}>
                      {n.pinned && <Pin size={13} style={{ color: 'var(--accent)' }} />}
                      <span className="eyebrow">{n.kind} · {n.unit}</span>
                    </div>
                    <div className="serif" style={{ fontSize: 19, lineHeight: 1.25 }}>{n.title}</div>
                    <div className="muted" style={{ fontSize: 13.5, marginTop: 3 }}>{n.summary}</div>
                    <div className="faint" style={{ fontSize: 12.5, marginTop: 6 }}>{n.author} · {fmtDate(n.published_at)}</div>
                  </Link>
                ))}
              </div>
            </section>
          )}
        </div>

        <aside className="stack" style={{ gap: 28 }}>
          <section>
            <div className="section-title"><h2>Your requests</h2></div>
            <div className="panel ruled">
              {mine?.length === 0 && <Empty title="No requests yet">Leave, events and other requests you submit are tracked here.</Empty>}
              {mine?.slice(0, 6).map((r) => (
                <Link key={r.id} to={`/approvals/${r.id}`} style={{ display: 'block', padding: '12px 16px' }}>
                  <div className="row between" style={{ gap: 8 }}>
                    <span style={{ fontWeight: 500, fontSize: 13.5, minWidth: 0, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{r.title.split(' · ')[0]}</span>
                    <StatusTag status={r.status} />
                  </div>
                  <div className="faint" style={{ fontSize: 12.5 }}>{r.current ? `With: ${r.current}` : `Updated ${ago(r.updated_at)}`}</div>
                </Link>
              ))}
            </div>
          </section>

          {data?.admin && (
            <section>
              <div className="section-title">
                <h2>School pulse</h2>
                <Link to="/admin" className="link" style={{ fontSize: 13 }}>Security</Link>
              </div>
              <div className="panel panel-pad stack">
                <div className="chart-bars" aria-label="Requests per day, last 14 days">
                  {data.admin.requests_by_day.map((d) => {
                    const max = Math.max(1, ...data.admin!.requests_by_day.map((x) => x.requests))
                    return <div key={d.day} title={`${fmtDate(d.day)}: ${d.requests}`} style={{ height: `${(d.requests / max) * 100}%` }} />
                  })}
                </div>
                <div className="faint" style={{ fontSize: 12 }}>Requests raised, last 14 days</div>
                <dl className="kv" style={{ gridTemplateColumns: '1fr auto' }}>
                  <dt>Active people</dt><dd className="num">{data.admin.active_users}</dd>
                  <dt>Online in the last hour</dt><dd className="num">{data.admin.online_users}</dd>
                  <dt>Requests in flight</dt><dd className="num">{data.admin.requests_in_flight}</dd>
                  <dt>Failed sign-ins, 24h</dt><dd className="num" style={{ color: data.admin.failed_logins_24h > 10 ? 'var(--bad)' : undefined }}>{data.admin.failed_logins_24h}</dd>
                </dl>
              </div>
            </section>
          )}
        </aside>
      </div>
    </main>
  )
}
