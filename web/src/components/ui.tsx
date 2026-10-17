import { createContext, ReactNode, useCallback, useContext, useEffect, useRef, useState } from 'react'
import { AlertTriangle, Check, CircleAlert, Info, Loader2, X } from 'lucide-react'

// ------------------------------------------------------------------ format --

export function initials(name: string) {
  const parts = name.replace(/^(dr|mr|mrs|ms)\.?\s+/i, '').split(/\s+/).filter(Boolean)
  return ((parts[0]?.[0] ?? '') + (parts.length > 1 ? parts[parts.length - 1][0] : '')).toUpperCase()
}

function hueOf(s: string) {
  let h = 0
  for (const c of s) h = (h * 31 + c.charCodeAt(0)) >>> 0
  return h % 360
}

export function Avatar({ name, size }: { name: string; size?: 'sm' | 'lg' }) {
  const h = hueOf(name)
  return (
    <span className={`avatar ${size ?? ''}`} style={{ background: `oklch(0.52 0.07 ${h})` }} aria-hidden>
      {initials(name)}
    </span>
  )
}

const rtf = new Intl.RelativeTimeFormat('en', { numeric: 'auto' })
export function ago(iso?: string | null) {
  if (!iso) return '—'
  const s = (new Date(iso).getTime() - Date.now()) / 1000
  const abs = Math.abs(s)
  if (abs < 45) return 'just now'
  if (abs < 3600) return rtf.format(Math.round(s / 60), 'minute')
  if (abs < 86400) return rtf.format(Math.round(s / 3600), 'hour')
  if (abs < 86400 * 7) return rtf.format(Math.round(s / 86400), 'day')
  return fmtDate(iso)
}

export function fmtDate(iso?: string | null, withTime = false) {
  if (!iso) return '—'
  const d = new Date(iso)
  return d.toLocaleDateString('en-GB', {
    day: 'numeric',
    month: 'short',
    year: d.getFullYear() === new Date().getFullYear() ? undefined : 'numeric',
    ...(withTime ? { hour: '2-digit', minute: '2-digit' } : {}),
  })
}

export function titleCase(s: string) {
  return s.replace(/_/g, ' ').replace(/^\w/, (c) => c.toUpperCase())
}

// ----------------------------------------------------------------- status --

const STATUS: Record<string, [string, string]> = {
  in_progress: ['In review', 'info'],
  in_review: ['In review', 'info'],
  pending: ['Awaiting', 'warn'],
  approved: ['Approved', 'ok'],
  published: ['Published', 'ok'],
  rejected: ['Declined', 'bad'],
  returned: ['Sent back', 'warn'],
  cancelled: ['Cancelled', ''],
  withdrawn: ['Not needed', ''],
  draft: ['Draft', ''],
  archived: ['Archived', ''],
  submitted: ['Submitted', 'info'],
  active: ['Active', 'ok'],
  disabled: ['Disabled', 'bad'],
  retired: ['Retired', ''],
  passed: ['Passed', 'ok'],
  waiting: ['Waiting', 'warn'],
  removed: ['Removed', 'bad'],
}

export function StatusTag({ status }: { status: string }) {
  const [label, tone] = STATUS[status] ?? [titleCase(status), '']
  return (
    <span className={`tag ${tone}`}>
      <span className="dot" />
      {label}
    </span>
  )
}

// ----------------------------------------------------------------- layout --

export function PageHead({ eyebrow, title, children, actions }: { eyebrow?: string; title: ReactNode; children?: ReactNode; actions?: ReactNode }) {
  return (
    <header className="page-head">
      <div>
        {eyebrow && <div className="eyebrow">{eyebrow}</div>}
        <h1>{title}</h1>
        {children && <p>{children}</p>}
      </div>
      {actions && <div className="actions">{actions}</div>}
    </header>
  )
}

export function Empty({ title, children, action }: { title: string; children?: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty">
      <h3>{title}</h3>
      {children && <p>{children}</p>}
      {action && <div style={{ marginTop: 16 }}>{action}</div>}
    </div>
  )
}

export function Spinner({ label = 'Loading' }: { label?: string }) {
  return (
    <div className="row faint" style={{ padding: 24, justifyContent: 'center' }} role="status">
      <Loader2 className="spin" size={16} /> {label}…
    </div>
  )
}

export function Notice({ tone = 'info', children }: { tone?: 'info' | 'warn' | 'bad' | 'ok'; children: ReactNode }) {
  const Icon = tone === 'bad' ? CircleAlert : tone === 'warn' ? AlertTriangle : tone === 'ok' ? Check : Info
  return (
    <div className={`notice ${tone}`} role={tone === 'bad' ? 'alert' : undefined}>
      <Icon />
      <div>{children}</div>
    </div>
  )
}

export function Field({ label, hint, required, children, className }: { label: string; hint?: ReactNode; required?: boolean; children: ReactNode; className?: string }) {
  return (
    <div className={`field ${className ?? ''}`}>
      <label>
        {label}
        {required && <span className="req">*</span>}
      </label>
      {children}
      {hint && <div className="hint">{hint}</div>}
    </div>
  )
}

export function Toggle({ on, onChange, disabled, label }: { on: boolean; onChange: (v: boolean) => void; disabled?: boolean; label: string }) {
  return <button type="button" role="switch" aria-checked={on} aria-label={label} className={`toggle ${on ? 'on' : ''}`} disabled={disabled} onClick={() => onChange(!on)} />
}

export function BoxCheck({ on, onChange, label, disabled }: { on: boolean; onChange: (v: boolean) => void; label: string; disabled?: boolean }) {
  return (
    <button type="button" role="checkbox" aria-checked={on} aria-label={label} disabled={disabled} className={`box-check ${on ? 'on' : ''}`} onClick={() => onChange(!on)}>
      {on && <Check />}
    </button>
  )
}

// ------------------------------------------------------------------ modal --

export function Modal({ title, description, onClose, children, footer, wide }: { title: string; description?: ReactNode; onClose: () => void; children: ReactNode; footer?: ReactNode; wide?: boolean }) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('keydown', onKey)
    const first = ref.current?.querySelector<HTMLElement>('input, select, textarea, button:not(.modal-x)')
    first?.focus()
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])
  return (
    <div className="modal-scrim" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className={`modal ${wide ? 'wide' : ''}`} role="dialog" aria-modal aria-label={title} ref={ref}>
        <div className="modal-head row between" style={{ alignItems: 'flex-start' }}>
          <div>
            <h2>{title}</h2>
            {description && <p>{description}</p>}
          </div>
          <button className="btn ghost icon sm modal-x" onClick={onClose} aria-label="Close">
            <X />
          </button>
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-foot">{footer}</div>}
      </div>
    </div>
  )
}

// ------------------------------------------------------------------ toast --

type Toast = { id: number; text: string; tone?: 'bad' }
const ToastCtx = createContext<(text: string, tone?: 'bad') => void>(() => {})

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([])
  const push = useCallback((text: string, tone?: 'bad') => {
    const id = Date.now() + Math.random()
    setToasts((t) => [...t, { id, text, tone }])
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4200)
  }, [])
  return (
    <ToastCtx.Provider value={push}>
      {children}
      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={`toast ${t.tone ?? ''}`}>
            {t.text}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  )
}

export const useToast = () => useContext(ToastCtx)

export function errorText(e: unknown) {
  return e instanceof Error ? e.message : 'Something went wrong.'
}

/** Minimal, safe markdown: paragraphs, **bold**, *italic*, lists, headings. */
export function Markdown({ text }: { text: string }) {
  const blocks = text.split(/\n{2,}/)
  const inline = (s: string, k: number) => {
    const parts: ReactNode[] = []
    const re = /(\*\*[^*]+\*\*|\*[^*]+\*)/g
    let last = 0
    let m: RegExpExecArray | null
    let i = 0
    while ((m = re.exec(s))) {
      if (m.index > last) parts.push(s.slice(last, m.index))
      const t = m[0]
      parts.push(t.startsWith('**') ? <strong key={`${k}-${i++}`}>{t.slice(2, -2)}</strong> : <em key={`${k}-${i++}`}>{t.slice(1, -1)}</em>)
      last = m.index + t.length
    }
    if (last < s.length) parts.push(s.slice(last))
    return parts
  }
  return (
    <div className="prose">
      {blocks.map((b, i) => {
        const lines = b.split('\n')
        if (lines.every((l) => /^\s*[-*]\s+/.test(l))) {
          return <ul key={i}>{lines.map((l, j) => <li key={j}>{inline(l.replace(/^\s*[-*]\s+/, ''), j)}</li>)}</ul>
        }
        if (b.startsWith('## ')) return <h2 key={i}>{inline(b.slice(3), i)}</h2>
        if (b.startsWith('### ')) return <h3 key={i}>{inline(b.slice(4), i)}</h3>
        return <p key={i}>{lines.map((l, j) => <span key={j}>{inline(l, j)}{j < lines.length - 1 && <br />}</span>)}</p>
      })}
    </div>
  )
}
