import { useEffect, useState } from 'react'
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowLeft, Pencil, Pin, Plus, Send } from 'lucide-react'
import { api, qs } from '../lib/api'
import { useSession } from '../lib/session'
import { unitLabel, unitsWithPerm, useUnits } from '../lib/units'
import { Empty, Field, Markdown, Notice, PageHead, Spinner, StatusTag, ago, errorText, fmtDate, useToast } from '../components/ui'

interface Item {
  id: string; kind: string; title: string; summary: string; status: string; pinned: boolean; unit: string; unit_id: string
  author: string; author_id: string; published_at: string | null; updated_at: string; approval_request_id: string | null; body?: string
}

const KINDS = [['', 'All'], ['announcement', 'Announcements'], ['circular', 'Circulars'], ['page', 'Pages']]

export default function Notices() {
  const { can } = useSession()
  const [params, setParams] = useSearchParams()
  const kind = params.get('kind') ?? ''
  const view = params.get('view') ?? 'published'
  const { data } = useQuery({ queryKey: ['content', kind], queryFn: () => api.get<Item[]>(`/content${qs({ kind })}`), enabled: view === 'published' })
  const { data: mine } = useQuery({ queryKey: ['content-mine'], queryFn: () => api.get<Item[]>('/content/mine'), enabled: can('content.create') })

  return (
    <main className="content">
      <PageHead
        eyebrow="Notices"
        title="Announcements & circulars"
        actions={can('content.create') && <Link to="/notices/new" className="btn primary"><Plus /> Write a notice</Link>}
      >
        Official word from the school. Notices are addressed to a group and reach everyone in it.
      </PageHead>
      {can('content.create') && (
        <div className="tabs">
          <button className={view === 'published' ? 'on' : ''} onClick={() => setParams({ view: 'published' })}>Published</button>
          <button className={view === 'mine' ? 'on' : ''} onClick={() => setParams({ view: 'mine' })}>My drafts & submissions <span className="n">{mine?.length ?? ''}</span></button>
        </div>
      )}
      {view === 'published' ? (
        <>
          <div className="segmented" style={{ marginBottom: 18 }}>
            {KINDS.map(([k, l]) => <button key={k} className={kind === k ? 'on' : ''} onClick={() => setParams(k ? { kind: k } : {})}>{l}</button>)}
          </div>
          <div className="panel ruled">
            {!data && <Spinner />}
            {data?.length === 0 && <Empty title="No notices yet" />}
            {data?.map((n) => {
              const d = new Date(n.published_at ?? n.updated_at)
              return (
                <Link to={`/notices/${n.id}`} key={n.id} className="notice-row">
                  <div className="notice-date">
                    <div className="d">{d.getDate()}</div>
                    <div className="m">{d.toLocaleDateString('en-GB', { month: 'short' })}</div>
                  </div>
                  <div style={{ minWidth: 0 }}>
                    <div className="row" style={{ gap: 8, marginBottom: 4 }}>
                      {n.pinned && <Pin size={13} style={{ color: 'var(--accent)' }} />}
                      <span className="eyebrow">{n.kind} · {n.unit}</span>
                    </div>
                    <h3>{n.title}</h3>
                    <div className="muted" style={{ marginTop: 4 }}>{n.summary}</div>
                  </div>
                  <div className="faint" style={{ fontSize: 12.5, textAlign: 'right' }}>{n.author}</div>
                </Link>
              )
            })}
          </div>
        </>
      ) : (
        <MyContent rows={mine} />
      )}
    </main>
  )
}

function MyContent({ rows }: { rows?: Item[] }) {
  const nav = useNavigate()
  if (!rows) return <Spinner />
  if (rows.length === 0) return <div className="panel"><Empty title="No drafts" action={<Link to="/notices/new" className="btn">Write one</Link>} /></div>
  return (
    <div className="panel table-wrap">
      <table className="table">
        <thead><tr><th>Title</th><th>Audience</th><th>Status</th><th>Updated</th></tr></thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.id} className="clickable" onClick={() => nav(r.status === 'draft' || r.status === 'rejected' ? `/notices/${r.id}/edit` : `/notices/${r.id}`)}>
              <td style={{ fontWeight: 500 }}>{r.title}<div className="faint" style={{ fontSize: 12.5 }}>{r.kind}</div></td>
              <td>{r.unit}</td>
              <td><StatusTag status={r.status} /></td>
              <td className="faint">{ago(r.updated_at)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function NoticeView() {
  const { id } = useParams()
  const { me, can } = useSession()
  const { data, error } = useQuery({ queryKey: ['content-item', id], queryFn: () => api.get<Item>(`/content/${id}`) })
  if (error) return <main className="content"><Notice tone="bad">{errorText(error)}</Notice></main>
  if (!data) return <main className="content"><Spinner /></main>
  const editable = data.author_id === me.user.id || can('content.publish')
  return (
    <main className="content">
      <Link to="/notices" className="row faint" style={{ gap: 6, fontSize: 13, marginBottom: 14 }}><ArrowLeft size={14} /> Notices</Link>
      <article style={{ maxWidth: 760 }}>
        <div className="eyebrow">{data.kind} · for {data.unit}</div>
        <h1 className="serif" style={{ fontSize: 40, lineHeight: 1.08, margin: '10px 0 12px', fontWeight: 500, letterSpacing: '-0.02em' }}>{data.title}</h1>
        {data.summary && <p className="muted serif" style={{ fontSize: 20, lineHeight: 1.4, margin: '0 0 16px' }}>{data.summary}</p>}
        <div className="row wrap" style={{ gap: 10, paddingBottom: 18, marginBottom: 24, borderBottom: '1px solid var(--rule)' }}>
          <span>{data.author}</span>
          <span className="faint">· {fmtDate(data.published_at ?? data.updated_at)}</span>
          {data.status !== 'published' && <StatusTag status={data.status} />}
          <span className="spacer" />
          {data.approval_request_id && <Link className="btn sm" to={`/approvals/${data.approval_request_id}`}>View approval</Link>}
          {editable && data.status !== 'in_review' && <Link className="btn sm" to={`/notices/${data.id}/edit`}><Pencil /> Edit</Link>}
        </div>
        <div style={{ fontSize: 16, lineHeight: 1.7 }}>
          <Markdown text={data.body ?? ''} />
        </div>
      </article>
    </main>
  )
}

export function NoticeEditor() {
  const { id } = useParams()
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const { me, can } = useSession()
  const { data: units } = useUnits()
  const allowed = unitsWithPerm(units, me.grants, 'content.create')
  const { data: existing } = useQuery({ queryKey: ['content-item', id], queryFn: () => api.get<Item>(`/content/${id}`), enabled: !!id })
  const [form, setForm] = useState({ kind: 'announcement', title: '', summary: '', body: '', org_unit_id: '', pinned: false })
  const [preview, setPreview] = useState(false)
  useEffect(() => {
    if (existing) setForm({ kind: existing.kind, title: existing.title, summary: existing.summary, body: existing.body ?? '', org_unit_id: existing.unit_id, pinned: existing.pinned })
  }, [existing])
  const unit = form.org_unit_id || allowed[0]?.id || ''
  const canPublish = me.grants.some((g) => g.perms.includes('content.publish'))

  const save = useMutation({
    mutationFn: async (submit: boolean) => {
      const body = { ...form, org_unit_id: unit }
      const cid = id ? (await api.put(`/content/${id}`, body), id) : (await api.post<{ id: string }>('/content', body)).id
      if (submit) return { cid, r: await api.post<{ status: string }>(`/content/${cid}/submit`) }
      return { cid, r: null }
    },
    onSuccess: ({ cid, r }) => {
      qc.invalidateQueries({ queryKey: ['content'] })
      qc.invalidateQueries({ queryKey: ['content-mine'] })
      qc.invalidateQueries({ queryKey: ['content-item', cid] })
      toast(r?.status === 'published' ? 'Published.' : r ? 'Sent for approval.' : 'Draft saved.')
      nav(r ? `/notices/${cid}` : `/notices/${cid}/edit`, { replace: true })
    },
    onError: (e) => toast(errorText(e), 'bad'),
  })

  if (!can('content.create')) return <main className="content"><Notice tone="warn">You can't write notices.</Notice></main>
  const set = (k: string, v: unknown) => setForm((f) => ({ ...f, [k]: v }))
  return (
    <main className="content">
      <Link to="/notices?view=mine" className="row faint" style={{ gap: 6, fontSize: 13, marginBottom: 14 }}><ArrowLeft size={14} /> Notices</Link>
      <PageHead eyebrow={id ? 'Edit notice' : 'New notice'} title={form.title || 'Untitled notice'}>
        {canPublish ? 'You can publish directly.' : 'When you submit, it goes through the publishing workflow before anyone sees it.'}
      </PageHead>
      <div className="split">
        <div className="panel panel-pad stack">
          <Field label="Title" required><input className="input" value={form.title} onChange={(e) => set('title', e.target.value)} maxLength={200} /></Field>
          <Field label="Summary" hint="One line shown in lists."><input className="input" value={form.summary} onChange={(e) => set('summary', e.target.value)} maxLength={500} /></Field>
          <div className="row between">
            <span className="label">Body</span>
            <div className="segmented">
              <button className={!preview ? 'on' : ''} onClick={() => setPreview(false)}>Write</button>
              <button className={preview ? 'on' : ''} onClick={() => setPreview(true)}>Preview</button>
            </div>
          </div>
          {preview ? (
            <div style={{ minHeight: 280, padding: '4px 2px' }}><Markdown text={form.body || '_Nothing yet._'} /></div>
          ) : (
            <textarea className="textarea" style={{ minHeight: 280, fontSize: 15 }} value={form.body} onChange={(e) => set('body', e.target.value)} placeholder={'Write in plain text.\n\nUse **bold**, *italic*, “- ” for lists and “## ” for headings.'} />
          )}
        </div>
        <aside className="panel panel-pad stack">
          <Field label="Type">
            <select className="select" value={form.kind} onChange={(e) => set('kind', e.target.value)}>
              <option value="announcement">Announcement</option>
              <option value="circular">Circular</option>
              <option value="page">Page</option>
            </select>
          </Field>
          <Field label="Audience" hint="Everyone in this group and above it will see it.">
            <select className="select" value={unit} onChange={(e) => set('org_unit_id', e.target.value)}>
              {allowed.map((u) => <option key={u.id} value={u.id}>{unitLabel(u)}</option>)}
            </select>
          </Field>
          {canPublish && (
            <label className="check"><input type="checkbox" checked={form.pinned} onChange={(e) => set('pinned', e.target.checked)} /> Pin to the top</label>
          )}
          <hr className="hr" />
          <button className="btn" disabled={!form.title.trim() || save.isPending} onClick={() => save.mutate(false)}>Save draft</button>
          <button className="btn primary" disabled={!form.title.trim() || save.isPending} onClick={() => save.mutate(true)}><Send /> {canPublish ? 'Publish' : 'Submit for approval'}</button>
          {existing?.status === 'rejected' && <Notice tone="warn">This was declined. Edit it and submit again.</Notice>}
        </aside>
      </div>
    </main>
  )
}
