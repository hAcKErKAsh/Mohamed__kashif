import type { FieldDef } from '../lib/types'
import { Field } from './ui'

/** Renders any school-defined form from its field list. */
export function FormRenderer({ fields, value, onChange, disabled }: {
  fields: FieldDef[]
  value: Record<string, unknown>
  onChange: (v: Record<string, unknown>) => void
  disabled?: boolean
}) {
  const set = (k: string, v: unknown) => onChange({ ...value, [k]: v })
  return (
    <div className="form-grid">
      {fields.map((f) => {
        const v = value[f.key]
        const full = f.type === 'textarea' || f.type === 'checkbox'
        if (f.type === 'checkbox') {
          return (
            <label key={f.key} className="check full">
              <input type="checkbox" checked={v === true} disabled={disabled} onChange={(e) => set(f.key, e.target.checked)} />
              <span>
                {f.label}
                {f.required && <span style={{ color: 'var(--bad)' }}> *</span>}
                {f.help && <span className="faint" style={{ display: 'block', fontSize: 12.5 }}>{f.help}</span>}
              </span>
            </label>
          )
        }
        return (
          <Field key={f.key} label={f.label} required={f.required} hint={f.help} className={full ? 'full' : ''}>
            {f.type === 'textarea' ? (
              <textarea className="textarea" value={(v as string) ?? ''} disabled={disabled} onChange={(e) => set(f.key, e.target.value)} required={f.required} />
            ) : f.type === 'select' ? (
              <select className="select" value={(v as string) ?? ''} disabled={disabled} onChange={(e) => set(f.key, e.target.value)} required={f.required}>
                <option value="">Choose…</option>
                {f.options?.map((o) => <option key={o} value={o}>{o}</option>)}
              </select>
            ) : (
              <input
                className="input"
                type={f.type === 'number' ? 'number' : f.type === 'date' ? 'date' : f.type === 'email' ? 'email' : 'text'}
                value={v === undefined || v === null ? '' : String(v)}
                min={f.min}
                max={f.max}
                step={f.type === 'number' ? 'any' : undefined}
                disabled={disabled}
                required={f.required}
                onChange={(e) => set(f.key, f.type === 'number' ? (e.target.value === '' ? '' : Number(e.target.value)) : e.target.value)}
              />
            )}
          </Field>
        )
      })}
    </div>
  )
}

export function DataTable({ data, fields }: { data: Record<string, unknown>; fields?: FieldDef[] | null }) {
  const keys = fields?.map((f) => f.key).filter((k) => k in data) ?? Object.keys(data)
  const label = (k: string) => fields?.find((f) => f.key === k)?.label ?? k.replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase())
  const show = (v: unknown) => (typeof v === 'boolean' ? (v ? 'Yes' : 'No') : v === null || v === undefined || v === '' ? '—' : String(v))
  if (keys.length === 0) return <div className="faint">No details.</div>
  return (
    <dl className="kv">
      {keys.map((k) => (
        <div key={k} style={{ display: 'contents' }}>
          <dt>{label(k)}</dt>
          <dd style={{ whiteSpace: 'pre-wrap' }}>{show(data[k])}</dd>
        </div>
      ))}
    </dl>
  )
}
