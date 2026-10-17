import { useState } from 'react'
import { Link, useNavigate, useParams } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowLeft, ArrowRight, GitBranch, Send } from 'lucide-react'
import { api } from '../lib/api'
import type { FieldDef } from '../lib/types'
import { useSession } from '../lib/session'
import { unitLabel, unitsWithPerm, useUnits } from '../lib/units'
import { FormRenderer } from '../components/FormRenderer'
import { Empty, Field, Notice, PageHead, Spinner, StatusTag, ago, errorText, useToast } from '../components/ui'

interface FormSummary { id: string; key: string; name: string; description: string; field_count: number; has_workflow: boolean }
interface Submission { id: string; form: string; status: string; created_at: string; request_id: string | null }

export default function Forms() {
  const { data } = useQuery({ queryKey: ['forms'], queryFn: () => api.get<FormSummary[]>('/forms') })
  const { data: subs } = useQuery({ queryKey: ['my-submissions'], queryFn: () => api.get<Submission[]>('/forms/submissions/mine') })
  const nav = useNavigate()
  return (
    <main className="content">
      <PageHead eyebrow="Requests & forms" title="What do you need?">
        Pick a form. Each one already knows who needs to approve it, so you don't have to chase anyone.
      </PageHead>
      {!data && <Spinner />}
      {data?.length === 0 && <div className="panel"><Empty title="No forms for your role yet">Your administrator can add forms in the form builder.</Empty></div>}
      <div className="grid-3" style={{ marginBottom: 36 }}>
        {data?.map((f) => (
          <Link key={f.id} to={`/forms/${f.key}`} className="panel panel-pad stack" style={{ gap: 8 }}>
            <div className="serif" style={{ fontSize: 21, lineHeight: 1.2 }}>{f.name}</div>
            <div className="muted" style={{ fontSize: 13.5, flex: 1 }}>{f.description}</div>
            <div className="row between faint" style={{ fontSize: 12.5 }}>
              <span className="row" style={{ gap: 6 }}>{f.has_workflow && <GitBranch size={13} />}{f.has_workflow ? 'Routed for approval' : 'Recorded only'}</span>
              <ArrowRight size={14} />
            </div>
          </Link>
        ))}
      </div>
      <div className="section-title"><h2>Your submissions</h2></div>
      <div className="panel table-wrap">
        {subs?.length === 0 ? <Empty title="Nothing submitted yet" /> : (
          <table className="table">
            <thead><tr><th>Form</th><th>Status</th><th>Submitted</th></tr></thead>
            <tbody>
              {subs?.map((s) => (
                <tr key={s.id} className={s.request_id ? 'clickable' : ''} onClick={() => s.request_id && nav(`/approvals/${s.request_id}`)}>
                  <td style={{ fontWeight: 500 }}>{s.form}</td>
                  <td><StatusTag status={s.status} /></td>
                  <td className="faint">{ago(s.created_at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </main>
  )
}

export function FormFill() {
  const { key } = useParams()
  const nav = useNavigate()
  const qc = useQueryClient()
  const toast = useToast()
  const { me } = useSession()
  const { data: units } = useUnits()
  const { data: form, error } = useQuery({ queryKey: ['form', key], queryFn: () => api.get<{ id: string; name: string; description: string; fields: FieldDef[]; workflow_key: string | null }>(`/forms/${key}`) })
  const [value, setValue] = useState<Record<string, unknown>>({})
  const [unit, setUnit] = useState('')
  const allowed = unitsWithPerm(units, me.grants, 'forms.submit').filter((u) => me.grants.some((g) => g.org_unit_id === u.id))
  const submit = useMutation({
    mutationFn: () => api.post<{ request_id: string | null }>(`/forms/${key}/submit`, { data: value, org_unit_id: unit || undefined }),
    onSuccess: (r) => {
      qc.invalidateQueries({ queryKey: ['my-requests'] })
      qc.invalidateQueries({ queryKey: ['my-submissions'] })
      qc.invalidateQueries({ queryKey: ['dashboard'] })
      toast('Submitted. You can follow its progress here.')
      nav(r.request_id ? `/approvals/${r.request_id}` : '/forms')
    },
    onError: (e) => toast(errorText(e), 'bad'),
  })
  if (error) return <main className="content"><Notice tone="bad">{errorText(error)}</Notice></main>
  if (!form) return <main className="content"><Spinner /></main>
  return (
    <main className="content">
      <Link to="/forms" className="row faint" style={{ gap: 6, fontSize: 13, marginBottom: 14 }}><ArrowLeft size={14} /> Forms</Link>
      <PageHead eyebrow="Request" title={form.name}>{form.description}</PageHead>
      <form className="panel" style={{ maxWidth: 760 }} onSubmit={(e) => { e.preventDefault(); submit.mutate() }}>
        <div className="panel-pad stack" style={{ gap: 18 }}>
          {allowed.length > 1 && (
            <Field label="On behalf of" hint="Decides who approves it.">
              <select className="select" value={unit} onChange={(e) => setUnit(e.target.value)}>
                <option value="">My main group</option>
                {allowed.map((u) => <option key={u.id} value={u.id}>{unitLabel(u)}</option>)}
              </select>
            </Field>
          )}
          <FormRenderer fields={form.fields} value={value} onChange={setValue} />
        </div>
        <div className="panel-foot">
          <Link to="/forms" className="btn ghost">Cancel</Link>
          <button className="btn primary" disabled={submit.isPending}><Send /> Submit</button>
        </div>
      </form>
    </main>
  )
}
