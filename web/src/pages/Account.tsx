import { FormEvent, useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { ShieldCheck } from 'lucide-react'
import { api } from '../lib/api'
import { useSession } from '../lib/session'
import { FormRenderer } from '../components/FormRenderer'
import { Avatar, Field, Notice, PageHead, ago, errorText, useToast } from '../components/ui'

interface Sess { id: string; ip: string | null; user_agent: string | null; created_at: string; last_seen_at: string; current: boolean }

function device(ua: string | null) {
  if (!ua) return 'Unknown device'
  const browser = /Edg\//.test(ua) ? 'Edge' : /Chrome\//.test(ua) ? 'Chrome' : /Firefox\//.test(ua) ? 'Firefox' : /Safari\//.test(ua) ? 'Safari' : 'Browser'
  const os = /Windows/.test(ua) ? 'Windows' : /Mac OS X/.test(ua) ? 'macOS' : /Android/.test(ua) ? 'Android' : /iPhone|iPad/.test(ua) ? 'iOS' : /Linux/.test(ua) ? 'Linux' : ''
  return `${browser}${os ? ` on ${os}` : ''}`
}

export default function Account() {
  const { me, refresh } = useSession()
  const toast = useToast()
  const qc = useQueryClient()
  const [profile, setProfile] = useState<Record<string, unknown>>(me.user.profile ?? {})
  const saveProfile = useMutation({ mutationFn: () => api.put('/me/profile', { profile }), onSuccess: () => { toast('Profile saved.'); refresh() }, onError: (e) => toast(errorText(e), 'bad') })
  const { data: sessions } = useQuery({ queryKey: ['my-sessions'], queryFn: () => api.get<Sess[]>('/auth/sessions') })
  const revoke = useMutation({ mutationFn: (id: string) => api.del(`/auth/sessions/${id}`), onSuccess: () => qc.invalidateQueries({ queryKey: ['my-sessions'] }) })

  return (
    <main className="content">
      <PageHead eyebrow="Account" title={me.user.full_name}>{me.user.email}</PageHead>
      {me.user.mfa_setup_required && <div style={{ marginBottom: 20 }}><Notice tone="warn">Your role requires two-step sign-in. Please turn it on below.</Notice></div>}
      <div className="grid-2">
        <section className="stack" style={{ gap: 20 }}>
          <div className="panel">
            <div className="panel-head"><h3>Roles</h3></div>
            <div className="panel-pad stack-sm">
              <div className="row" style={{ gap: 12, marginBottom: 6 }}><Avatar name={me.user.full_name} size="lg" /><div className="muted" style={{ fontSize: 13 }}>Your access comes from these roles. Ask your administrator if something is missing.</div></div>
              {me.grants.map((g) => (
                <div key={g.membership_id} className="row between" style={{ fontSize: 13.5 }}>
                  <span style={{ fontWeight: 500 }}>{g.role_name}</span>
                  <span className="faint">{g.org_unit_name}</span>
                </div>
              ))}
            </div>
          </div>
          {me.settings.profile_fields.length > 0 && (
            <form className="panel" onSubmit={(e) => { e.preventDefault(); saveProfile.mutate() }}>
              <div className="panel-head"><h3>Profile</h3></div>
              <div className="panel-pad"><FormRenderer fields={me.settings.profile_fields} value={profile} onChange={setProfile} /></div>
              <div className="panel-foot"><button className="btn primary" disabled={saveProfile.isPending}>Save</button></div>
            </form>
          )}
          <div className="panel">
            <div className="panel-head"><h3>Where you're signed in</h3></div>
            <div className="ruled">
              {sessions?.map((s) => (
                <div key={s.id} className="row between" style={{ padding: '12px 20px' }}>
                  <div>
                    <div style={{ fontWeight: 500, fontSize: 13.5 }}>{device(s.user_agent)} {s.current && <span className="tag accent" style={{ marginLeft: 6 }}>This device</span>}</div>
                    <div className="faint" style={{ fontSize: 12.5 }}>{s.ip ?? 'unknown'} · active {ago(s.last_seen_at)}</div>
                  </div>
                  {!s.current && <button className="btn sm" onClick={() => revoke.mutate(s.id)}>Sign out</button>}
                </div>
              ))}
            </div>
          </div>
        </section>
        <section className="stack" style={{ gap: 20 }}>
          <PasswordPanel />
          <MfaPanel />
        </section>
      </div>
    </main>
  )
}

function PasswordPanel() {
  const toast = useToast()
  const [f, setF] = useState({ current: '', next: '', confirm: '' })
  const [err, setErr] = useState('')
  const m = useMutation({
    mutationFn: () => api.post('/auth/password', { current_password: f.current, new_password: f.next }),
    onSuccess: () => { toast('Password changed. Other devices were signed out.'); setF({ current: '', next: '', confirm: '' }); setErr('') },
    onError: (e) => setErr(errorText(e)),
  })
  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (f.next !== f.confirm) return setErr("The new passwords don't match.")
    m.mutate()
  }
  return (
    <form className="panel" onSubmit={submit}>
      <div className="panel-head"><h3>Password</h3></div>
      <div className="panel-pad stack">
        {err && <Notice tone="bad">{err}</Notice>}
        <Field label="Current password"><input className="input" type="password" autoComplete="current-password" value={f.current} onChange={(e) => setF({ ...f, current: e.target.value })} required /></Field>
        <Field label="New password" hint="12+ characters. Long and memorable beats short and complex."><input className="input" type="password" autoComplete="new-password" value={f.next} onChange={(e) => setF({ ...f, next: e.target.value })} required /></Field>
        <Field label="Confirm"><input className="input" type="password" autoComplete="new-password" value={f.confirm} onChange={(e) => setF({ ...f, confirm: e.target.value })} required /></Field>
      </div>
      <div className="panel-foot"><button className="btn primary" disabled={m.isPending}>Change password</button></div>
    </form>
  )
}

function MfaPanel() {
  const { me, refresh } = useSession()
  const toast = useToast()
  const [setup, setSetup] = useState<{ secret: string; otpauth_url: string } | null>(null)
  const [code, setCode] = useState('')
  const [pw, setPw] = useState('')
  const [err, setErr] = useState('')
  const start = useMutation({ mutationFn: () => api.post<{ secret: string; otpauth_url: string }>('/auth/mfa/setup'), onSuccess: setSetup, onError: (e) => setErr(errorText(e)) })
  const enable = useMutation({ mutationFn: () => api.post('/auth/mfa/enable', { code }), onSuccess: () => { toast('Two-step sign-in is on.'); setSetup(null); refresh() }, onError: (e) => setErr(errorText(e)) })
  const disable = useMutation({ mutationFn: () => api.post('/auth/mfa/disable', { password: pw }), onSuccess: () => { toast('Two-step sign-in turned off.'); setPw(''); refresh() }, onError: (e) => setErr(errorText(e)) })
  return (
    <div className="panel">
      <div className="panel-head"><h3>Two-step sign-in</h3>{me.user.totp_enabled ? <span className="tag ok"><ShieldCheck size={12} /> On</span> : <span className="tag">Off</span>}</div>
      <div className="panel-pad stack">
        {err && <Notice tone="bad">{err}</Notice>}
        {me.user.totp_enabled ? (
          <>
            <p className="muted" style={{ margin: 0 }}>You'll be asked for a code from your authenticator app each time you sign in.</p>
            <Field label="Confirm your password to turn it off"><input className="input" type="password" value={pw} onChange={(e) => setPw(e.target.value)} /></Field>
            <div><button className="btn danger" disabled={!pw || disable.isPending} onClick={() => disable.mutate()}>Turn off</button></div>
          </>
        ) : setup ? (
          <>
            <p className="muted" style={{ margin: 0 }}>In Google Authenticator, Microsoft Authenticator or 1Password, add an account with this key:</p>
            <div className="mono" style={{ fontSize: 15, letterSpacing: '0.08em', background: 'var(--surface-2)', padding: '12px 14px', borderRadius: 4, wordBreak: 'break-all' }}>{setup.secret.match(/.{1,4}/g)?.join(' ')}</div>
            <a className="link" href={setup.otpauth_url} style={{ fontSize: 13 }}>Open in an authenticator app on this device</a>
            <Field label="Then enter the 6-digit code it shows"><input className="input mono" inputMode="numeric" maxLength={6} value={code} onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))} /></Field>
            <div><button className="btn primary" disabled={code.length !== 6 || enable.isPending} onClick={() => enable.mutate()}>Turn on</button></div>
          </>
        ) : (
          <>
            <p className="muted" style={{ margin: 0 }}>Protect your account with a code from your phone, so a stolen password alone isn't enough.</p>
            <div><button className="btn primary" onClick={() => start.mutate()} disabled={start.isPending}>Set up</button></div>
          </>
        )}
      </div>
    </div>
  )
}
