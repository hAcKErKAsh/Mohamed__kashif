import { Link } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { ShieldAlert, ShieldCheck } from 'lucide-react'
import { api } from '../../lib/api'
import { useSession } from '../../lib/session'
import { Notice, PageHead, Spinner, ago } from '../../components/ui'

interface AuditRow { seq: number; at: string; action: string; actor_name: string | null; ip: string | null; entity_type: string }
interface Dash { admin: null | { active_users: number; online_users: number; requests_in_flight: number; failed_logins_24h: number; locked_accounts: number } }

const SECURITY_ACTIONS = ['auth.login_failed', 'auth.locked', 'auth.mfa_failed', 'membership.granted', 'membership.revoked', 'role.updated', 'role.created', 'settings.updated', 'user.password_reset', 'session.revoked']

export default function Overview() {
  const { can } = useSession()
  const { data: dash } = useQuery({ queryKey: ['dashboard'], queryFn: () => api.get<Dash>('/dashboard') })
  const { data: verify } = useQuery({ queryKey: ['audit-verify'], queryFn: () => api.get<{ rows: number; intact: boolean }>('/admin/audit/verify'), enabled: can('admin.audit.view') })
  const { data: audit } = useQuery({ queryKey: ['audit', 'overview'], queryFn: () => api.get<AuditRow[]>('/admin/audit'), enabled: can('admin.audit.view') })
  const a = dash?.admin
  const security = audit?.filter((r) => SECURITY_ACTIONS.includes(r.action)).slice(0, 12)
  return (
    <main className="content">
      <PageHead eyebrow="Control room" title="Security overview">
        Who can do what, what changed, and whether anything looks wrong.
      </PageHead>
      {!a ? <Spinner /> : (
        <div className="panel stats" style={{ marginBottom: 24 }}>
          <div className="stat"><span className="eyebrow">Active accounts</span><span className="value">{a.active_users}</span><span className="sub">{a.online_users} online in the last hour</span></div>
          <div className="stat"><span className="eyebrow">Failed sign-ins</span><span className="value" style={{ color: a.failed_logins_24h > 10 ? 'var(--bad)' : undefined }}>{a.failed_logins_24h}</span><span className="sub">in the last 24 hours</span></div>
          <div className="stat"><span className="eyebrow">Locked accounts</span><span className="value" style={{ color: a.locked_accounts ? 'var(--warn)' : undefined }}>{a.locked_accounts}</span><span className="sub">after repeated failures</span></div>
          <div className="stat"><span className="eyebrow">Requests in flight</span><span className="value">{a.requests_in_flight}</span><span className="sub">across all workflows</span></div>
        </div>
      )}
      <div className="split">
        <section>
          <div className="section-title"><h2>Recent security events</h2><Link to="/admin/audit" className="link" style={{ fontSize: 13 }}>Full audit trail</Link></div>
          <div className="panel table-wrap">
            <table className="table">
              <thead><tr><th>Event</th><th>By</th><th>From</th><th>When</th></tr></thead>
              <tbody>
                {security?.length === 0 && <tr><td colSpan={4} className="faint">No security events yet.</td></tr>}
                {security?.map((r) => (
                  <tr key={r.seq}>
                    <td><span className={`tag ${r.action.includes('failed') || r.action.includes('locked') ? 'bad' : r.action.includes('role') || r.action.includes('membership') ? 'accent' : ''}`}>{r.action}</span></td>
                    <td>{r.actor_name ?? <span className="faint">unknown</span>}</td>
                    <td className="mono faint">{r.ip ?? '—'}</td>
                    <td className="faint">{ago(r.at)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </section>
        <aside className="stack" style={{ gap: 16 }}>
          {verify && (verify.intact ? (
            <Notice tone="ok"><strong>Audit trail intact.</strong> All {verify.rows.toLocaleString()} entries verify against their hash chain.</Notice>
          ) : (
            <Notice tone="bad"><strong>Audit trail tampered.</strong> The hash chain is broken. Investigate immediately.</Notice>
          ))}
          <div className="panel">
            <div className="panel-head"><h3>Controls in place</h3></div>
            <div className="panel-pad stack-sm" style={{ fontSize: 13.5 }}>
              {[
                'Argon2id password hashing, breach-list and length policy',
                'Account lockout and per-IP sign-in throttling',
                'Opaque, hashed session tokens in HttpOnly SameSite cookies',
                'CSRF tokens and origin checks on every change',
                'Optional or role-mandated TOTP two-step sign-in',
                'Scoped permissions follow the organisation tree',
                'No self-approval; no granting powers you lack',
                'Append-only, hash-chained audit log',
                'Strict Content-Security-Policy and security headers',
              ].map((c) => <div key={c} className="row" style={{ gap: 8, alignItems: 'flex-start' }}><ShieldCheck size={15} style={{ color: 'var(--ok)', flex: 'none', marginTop: 2 }} />{c}</div>)}
            </div>
          </div>
          <div className="panel panel-pad row" style={{ gap: 10 }}>
            <ShieldAlert size={18} style={{ color: 'var(--warn)' }} />
            <div style={{ fontSize: 13.5 }}>Require two-step sign-in for staff in <Link to="/admin/settings" className="link">Modules & policy</Link>.</div>
          </div>
        </aside>
      </div>
    </main>
  )
}
