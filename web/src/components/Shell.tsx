import { ReactNode, useEffect, useRef, useState } from 'react'
import { NavLink, useLocation, useNavigate } from 'react-router-dom'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import {
  Bell, BookOpen, ClipboardList, FileText, GitBranch, Home, KeyRound, LayoutGrid, LogOut, Menu, Moon, Network,
  ScrollText, Settings2, ShieldCheck, Stamp, Sun, Users, UsersRound, MessagesSquare, MonitorSmartphone,
} from 'lucide-react'
import { useSession } from '../lib/session'
import { api } from '../lib/api'
import { Avatar, ago } from './ui'

function NavItem({ to, icon: Icon, children, count, end }: { to: string; icon: typeof Home; children: ReactNode; count?: number; end?: boolean }) {
  return (
    <NavLink to={to} end={end} className={({ isActive }) => (isActive ? 'active' : '')}>
      <Icon />
      <span>{children}</span>
      {!!count && <span className="count">{count}</span>}
    </NavLink>
  )
}

export function Shell({ children }: { children: ReactNode }) {
  const { me, can, module, logout } = useSession()
  const [open, setOpen] = useState(false)
  const loc = useLocation()
  const nav = useNavigate()
  useEffect(() => setOpen(false), [loc.pathname])

  const { data: dash } = useQuery({ queryKey: ['dashboard'], queryFn: () => api.get<{ pending_tasks: number }>('/dashboard'), refetchInterval: 60_000 })

  const b = me.settings.branding
  const isAdmin = ['admin.users.manage', 'admin.roles.manage', 'admin.org.manage', 'admin.workflows.manage', 'admin.forms.manage', 'admin.settings.manage', 'admin.audit.view', 'admin.sessions.manage'].some(can)
  const primaryRole = me.grants[0]

  return (
    <div className="shell">
      {open && <div className="scrim" onClick={() => setOpen(false)} />}
      <aside className={`sidebar ${open ? 'open' : ''}`}>
        <div className="crest">
          <div className="crest-mark">{b.name.trim()[0] ?? 'L'}</div>
          <div style={{ minWidth: 0 }}>
            <div className="crest-name">{b.name}</div>
            <div className="crest-sub">{b.motto || b.tagline}</div>
          </div>
        </div>

        <nav className="nav" aria-label="Main">
          <div className="nav-label eyebrow">Everyday</div>
          <NavItem to="/" icon={Home} end>Today</NavItem>
          {module('approvals') && <NavItem to="/approvals" icon={Stamp} count={dash?.pending_tasks}>Approvals</NavItem>}
          {module('content') && can('content.view') && <NavItem to="/notices" icon={BookOpen}>Notices</NavItem>}
          {module('social') && can('social.view') && <NavItem to="/feed" icon={MessagesSquare}>Community</NavItem>}
          {module('forms') && can('forms.submit') && <NavItem to="/forms" icon={ClipboardList}>Requests & forms</NavItem>}
          {module('directory') && can('people.view') && <NavItem to="/people" icon={UsersRound}>People</NavItem>}
        </nav>

        {isAdmin && (
          <nav className="nav" aria-label="Administration">
            <div className="nav-label eyebrow">Control room</div>
            <NavItem to="/admin" icon={ShieldCheck} end>Security overview</NavItem>
            {can('admin.users.manage') && <NavItem to="/admin/users" icon={Users}>People & access</NavItem>}
            {can('admin.roles.manage') && <NavItem to="/admin/roles" icon={KeyRound}>Roles & permissions</NavItem>}
            {can('admin.org.manage') && <NavItem to="/admin/org" icon={Network}>Organisation</NavItem>}
            {can('admin.workflows.manage') && <NavItem to="/admin/workflows" icon={GitBranch}>Workflows</NavItem>}
            {can('admin.forms.manage') && <NavItem to="/admin/forms" icon={FileText}>Form builder</NavItem>}
            {can('admin.settings.manage') && <NavItem to="/admin/settings" icon={Settings2}>Modules & policy</NavItem>}
            {can('admin.sessions.manage') && <NavItem to="/admin/sessions" icon={MonitorSmartphone}>Live sessions</NavItem>}
            {can('admin.audit.view') && <NavItem to="/admin/audit" icon={ScrollText}>Audit trail</NavItem>}
          </nav>
        )}

        <div className="sidebar-foot">
          <Avatar name={me.user.full_name} />
          <button className="who link" style={{ textDecoration: 'none', color: 'inherit', textAlign: 'left' }} onClick={() => nav('/account')}>
            <div style={{ fontWeight: 500 }}>{me.user.full_name}</div>
            <div className="faint" style={{ fontSize: 12 }}>{primaryRole ? `${primaryRole.role_name} · ${primaryRole.org_unit_name}` : 'No role yet'}</div>
          </button>
          <button className="btn ghost icon sm" title="Sign out" aria-label="Sign out" onClick={() => logout()}>
            <LogOut />
          </button>
        </div>
      </aside>

      <div className="main">
        <div className="topbar">
          <button className="btn ghost icon mobile-only" aria-label="Open menu" onClick={() => setOpen(true)}>
            <Menu />
          </button>
          <div className="crumbs mobile-only">{b.name}</div>
          <div className="spacer" />
          <ThemeToggle />
          <Notifications />
          <NavLink to="/account" className="btn ghost icon" aria-label="Account" title="Account">
            <LayoutGrid />
          </NavLink>
        </div>
        {children}
      </div>
    </div>
  )
}

function ThemeToggle() {
  const [theme, setTheme] = useState<string>(() => document.documentElement.dataset.theme ?? (matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light'))
  const flip = () => {
    const next = theme === 'dark' ? 'light' : 'dark'
    document.documentElement.dataset.theme = next
    try {
      localStorage.setItem('lyceum-theme', next)
    } catch {
      /* private mode */
    }
    setTheme(next)
  }
  return (
    <button className="btn ghost icon" onClick={flip} aria-label="Toggle dark mode" title="Toggle theme">
      {theme === 'dark' ? <Sun /> : <Moon />}
    </button>
  )
}

interface Notif { id: string; title: string; link?: string; read: boolean; created_at: string }

function Notifications() {
  const { me, refresh } = useSession()
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)
  const qc = useQueryClient()
  const nav = useNavigate()
  const { data } = useQuery({ queryKey: ['notifications'], queryFn: () => api.get<Notif[]>('/notifications'), enabled: open })
  const markRead = useMutation({
    mutationFn: () => api.post('/notifications/read'),
    onSuccess: () => {
      qc.invalidateQueries({ queryKey: ['notifications'] })
      refresh()
    },
  })
  useEffect(() => {
    const close = (e: MouseEvent) => ref.current && !ref.current.contains(e.target as Node) && setOpen(false)
    document.addEventListener('mousedown', close)
    return () => document.removeEventListener('mousedown', close)
  }, [])
  const unread = me.unread_notifications
  return (
    <div style={{ position: 'relative' }} ref={ref}>
      <button className="btn ghost icon" aria-label={`Notifications${unread ? `, ${unread} unread` : ''}`} onClick={() => setOpen((o) => !o)} style={{ position: 'relative' }}>
        <Bell />
        {unread > 0 && <span style={{ position: 'absolute', top: 6, right: 7, width: 7, height: 7, borderRadius: 4, background: 'var(--bad)' }} />}
      </button>
      {open && (
        <div className="menu-pop">
          <div className="panel-head">
            <h3>Notifications</h3>
            {unread > 0 && <button className="link" style={{ fontSize: 12.5 }} onClick={() => markRead.mutate()}>Mark all read</button>}
          </div>
          <div style={{ maxHeight: 380, overflow: 'auto' }} className="ruled">
            {!data && <div className="faint" style={{ padding: 16 }}>Loading…</div>}
            {data?.length === 0 && <div className="faint" style={{ padding: 16 }}>Nothing yet.</div>}
            {data?.map((n) => (
              <button
                key={n.id}
                onClick={() => {
                  setOpen(false)
                  if (n.link) nav(n.link)
                }}
                style={{ display: 'block', width: '100%', textAlign: 'left', padding: '11px 16px', background: n.read ? 'transparent' : 'var(--accent-soft)', border: 0, cursor: 'pointer' }}
              >
                <div style={{ fontSize: 13.5 }}>{n.title}</div>
                <div className="faint" style={{ fontSize: 12 }}>{ago(n.created_at)}</div>
              </button>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
