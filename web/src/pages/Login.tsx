import { FormEvent, useEffect, useState } from 'react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { ArrowRight, Loader2 } from 'lucide-react'
import { api, setCsrf } from '../lib/api'
import { applyBranding } from '../lib/session'
import { Field, Notice, errorText } from '../components/ui'

const DEMO = import.meta.env.MODE === 'demo'
const DEMO_ACCOUNTS = [
  ['admin', 'Amira Haddad', 'Administrator · security'],
  ['headmaster', 'Dr. Rafael Okafor', 'Headmaster'],
  ['meera', 'Meera Pillai', 'Coordinator · Primary'],
  ['grace', 'Grace Lindqvist', 'Teacher · 4-A'],
  ['priya', 'Priya Shah', 'Parent of Aarav'],
  ['aarav', 'Aarav Shah', 'Student · 4-A'],
]

export function Login() {
  const qc = useQueryClient()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [code, setCode] = useState('')
  const [step, setStep] = useState<'password' | 'mfa'>('password')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const { data: pub } = useQuery({
    queryKey: ['public-settings'],
    queryFn: () => api.get<{ branding: { name: string; tagline: string; accent: string; motto?: string } }>('/settings/public'),
  })
  const b = pub?.branding
  useEffect(() => applyBranding(b?.accent, b?.name), [b])

  async function signIn(e?: FormEvent, asEmail?: string, asPassword?: string) {
    e?.preventDefault()
    setBusy(true)
    setError('')
    try {
      const r = await api.post<{ mfa_required: boolean; csrf_token: string }>('/auth/login', { email: asEmail ?? email, password: asPassword ?? password })
      setCsrf(r.csrf_token)
      if (r.mfa_required) setStep('mfa')
      else await qc.invalidateQueries({ queryKey: ['me'] })
    } catch (err) {
      setError(errorText(err))
    } finally {
      setBusy(false)
    }
  }

  async function verify(e: FormEvent) {
    e.preventDefault()
    setBusy(true)
    setError('')
    try {
      const r = await api.post<{ csrf_token: string }>('/auth/mfa/verify', { code })
      setCsrf(r.csrf_token)
      await qc.invalidateQueries({ queryKey: ['me'] })
    } catch (err) {
      setError(errorText(err))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="auth">
      <section className="auth-art">
        <div className="crest" style={{ padding: 0, position: 'relative' }}>
          <div className="crest-mark">{(b?.name ?? 'L')[0]}</div>
          <div>
            <div className="crest-name">{b?.name ?? 'Lyceum'}</div>
            <div style={{ opacity: 0.75, fontSize: 12 }}>{b?.tagline}</div>
          </div>
        </div>
        <blockquote>Every request, notice and decision in one quiet, accountable place.</blockquote>
        <div style={{ opacity: 0.7, fontSize: 12.5, position: 'relative' }}>Lyceum school platform · Access is monitored and recorded.</div>
      </section>

      <section className="auth-form">
        {step === 'password' ? (
          <form onSubmit={signIn} className="stack" style={{ gap: 18 }}>
            <div>
              <div className="eyebrow">Welcome back</div>
              <h1 style={{ marginTop: 8 }}>Sign in</h1>
            </div>
            {error && <Notice tone="bad">{error}</Notice>}
            <Field label="School email">
              <input className="input" type="email" autoComplete="username" required value={email} onChange={(e) => setEmail(e.target.value)} placeholder="name@school.org" />
            </Field>
            <Field label="Password">
              <input className="input" type="password" autoComplete="current-password" required value={password} onChange={(e) => setPassword(e.target.value)} />
            </Field>
            <button className="btn primary" style={{ height: 40 }} disabled={busy}>
              {busy ? <Loader2 className="spin" /> : null} Continue <ArrowRight />
            </button>
            <p className="faint" style={{ fontSize: 12.5, margin: 0 }}>Forgotten your password? Your school administrator can issue a one-time password.</p>
            {DEMO && (
              <div className="panel" style={{ marginTop: 6 }}>
                <div className="panel-head"><h3>Demo — sign in as</h3></div>
                <div className="ruled">
                  {DEMO_ACCOUNTS.map(([h, name, role]) => (
                    <button key={h} type="button" onClick={() => signIn(undefined, `${h}@northfield.school`, 'demo')} className="row" style={{ width: '100%', padding: '10px 16px', background: 'none', border: 0, cursor: 'pointer', textAlign: 'left' }}>
                      <span style={{ fontWeight: 500 }}>{name}</span>
                      <span className="faint" style={{ fontSize: 12.5 }}>{role}</span>
                      <ArrowRight size={14} style={{ marginLeft: 'auto', color: 'var(--ink-3)' }} />
                    </button>
                  ))}
                </div>
              </div>
            )}
          </form>
        ) : (
          <form onSubmit={verify} className="stack" style={{ gap: 18 }}>
            <div>
              <div className="eyebrow">Two-step sign-in</div>
              <h1 style={{ marginTop: 8 }}>Enter your code</h1>
              <p className="muted">Open your authenticator app and type the 6-digit code for {b?.name ?? 'Lyceum'}.</p>
            </div>
            {error && <Notice tone="bad">{error}</Notice>}
            <Field label="Code">
              <input className="input mono" inputMode="numeric" autoComplete="one-time-code" maxLength={6} required value={code} onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))} style={{ letterSpacing: '0.3em', fontSize: 18, height: 44 }} />
            </Field>
            <button className="btn primary" style={{ height: 40 }} disabled={busy || code.length !== 6}>
              Verify
            </button>
          </form>
        )}
      </section>
    </div>
  )
}

export function ForcePasswordChange({ name }: { name: string }) {
  const qc = useQueryClient()
  const [current, setCurrent] = useState('')
  const [next, setNext] = useState('')
  const [confirm, setConfirm] = useState('')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  async function submit(e: FormEvent) {
    e.preventDefault()
    if (next !== confirm) return setError("The new passwords don't match.")
    setBusy(true)
    setError('')
    try {
      await api.post('/auth/password', { current_password: current, new_password: next })
      await qc.invalidateQueries({ queryKey: ['me'] })
    } catch (err) {
      setError(errorText(err))
    } finally {
      setBusy(false)
    }
  }
  return (
    <div className="auth-form" style={{ minHeight: '100vh' }}>
      <form onSubmit={submit} className="stack" style={{ gap: 16 }}>
        <div>
          <div className="eyebrow">One more step</div>
          <h1 style={{ marginTop: 8 }}>Choose your own password</h1>
          <p className="muted">Welcome, {name}. You signed in with a one-time password. Pick something only you know — a short sentence of three or four words works well.</p>
        </div>
        {error && <Notice tone="bad">{error}</Notice>}
        <Field label="One-time password"><input className="input" type="password" autoComplete="current-password" value={current} onChange={(e) => setCurrent(e.target.value)} required /></Field>
        <Field label="New password" hint="At least 12 characters."><input className="input" type="password" autoComplete="new-password" value={next} onChange={(e) => setNext(e.target.value)} required /></Field>
        <Field label="Confirm new password"><input className="input" type="password" autoComplete="new-password" value={confirm} onChange={(e) => setConfirm(e.target.value)} required /></Field>
        <button className="btn primary" style={{ height: 40 }} disabled={busy}>Save and continue</button>
      </form>
    </div>
  )
}
