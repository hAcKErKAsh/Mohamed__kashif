import { lazy, Suspense } from 'react'
import { BrowserRouter, HashRouter, Navigate, Route, Routes } from 'react-router-dom'
import { SessionProvider, useMeQuery, useSession } from './lib/session'
import { Shell } from './components/Shell'
import { Spinner, Notice } from './components/ui'
import { Login, ForcePasswordChange } from './pages/Login'
import { Home } from './pages/Home'

const Feed = lazy(() => import('./pages/Feed'))
const Notices = lazy(() => import('./pages/Notices'))
const NoticeView = lazy(() => import('./pages/Notices').then((m) => ({ default: m.NoticeView })))
const NoticeEditor = lazy(() => import('./pages/Notices').then((m) => ({ default: m.NoticeEditor })))
const Approvals = lazy(() => import('./pages/Approvals'))
const RequestDetail = lazy(() => import('./pages/RequestDetail'))
const Forms = lazy(() => import('./pages/Forms'))
const FormFill = lazy(() => import('./pages/Forms').then((m) => ({ default: m.FormFill })))
const People = lazy(() => import('./pages/People'))
const Account = lazy(() => import('./pages/Account'))
const AdminOverview = lazy(() => import('./pages/admin/Overview'))
const AdminUsers = lazy(() => import('./pages/admin/Users'))
const AdminUser = lazy(() => import('./pages/admin/Users').then((m) => ({ default: m.UserDetail })))
const AdminRoles = lazy(() => import('./pages/admin/Roles'))
const AdminOrg = lazy(() => import('./pages/admin/Org'))
const AdminWorkflows = lazy(() => import('./pages/admin/Workflows'))
const AdminWorkflowEditor = lazy(() => import('./pages/admin/WorkflowEditor'))
const AdminForms = lazy(() => import('./pages/admin/FormBuilder'))
const AdminSettings = lazy(() => import('./pages/admin/Settings'))
const AdminAudit = lazy(() => import('./pages/admin/Audit'))
const AdminSessions = lazy(() => import('./pages/admin/Sessions'))

const Router = import.meta.env.MODE === 'demo' ? HashRouter : BrowserRouter

export function App() {
  const { data: me, isLoading, error } = useMeQuery()
  if (isLoading) return <Spinner />
  if (error) return <div style={{ padding: 32 }}><Notice tone="bad">The server isn't responding. Please try again in a moment.</Notice></div>
  if (!me) return <Login />
  if (me.user.must_change_password) return <ForcePasswordChange name={me.user.full_name} />
  return (
    <SessionProvider me={me}>
      <Router future={{ v7_startTransition: true, v7_relativeSplatPath: true }}>
        <Shell banner={import.meta.env.MODE === 'demo' ? <DemoBanner /> : null}>
          <Suspense fallback={<Spinner />}>
            <AppRoutes />
          </Suspense>
        </Shell>
      </Router>
    </SessionProvider>
  )
}

function DemoBanner() {
  return <div className="demo-banner">Interactive demo running entirely in your browser — changes reset when you reload. The full app runs on Rust + PostgreSQL.</div>
}

function Guard({ perm, children }: { perm: string; children: JSX.Element }) {
  const { can } = useSession()
  return can(perm) ? children : <Navigate to="/" replace />
}

function AppRoutes() {
  return (
    <Routes>
      <Route path="/" element={<Home />} />
      <Route path="/feed" element={<Feed />} />
      <Route path="/notices" element={<Notices />} />
      <Route path="/notices/new" element={<NoticeEditor />} />
      <Route path="/notices/:id" element={<NoticeView />} />
      <Route path="/notices/:id/edit" element={<NoticeEditor />} />
      <Route path="/approvals" element={<Approvals />} />
      <Route path="/approvals/:id" element={<RequestDetail />} />
      <Route path="/forms" element={<Forms />} />
      <Route path="/forms/:key" element={<FormFill />} />
      <Route path="/people" element={<People />} />
      <Route path="/account" element={<Account />} />
      <Route path="/admin" element={<AdminOverview />} />
      <Route path="/admin/users" element={<Guard perm="admin.users.manage"><AdminUsers /></Guard>} />
      <Route path="/admin/users/:id" element={<Guard perm="admin.users.manage"><AdminUser /></Guard>} />
      <Route path="/admin/roles" element={<Guard perm="admin.roles.manage"><AdminRoles /></Guard>} />
      <Route path="/admin/org" element={<Guard perm="admin.org.manage"><AdminOrg /></Guard>} />
      <Route path="/admin/workflows" element={<Guard perm="admin.workflows.manage"><AdminWorkflows /></Guard>} />
      <Route path="/admin/workflows/:id" element={<Guard perm="admin.workflows.manage"><AdminWorkflowEditor /></Guard>} />
      <Route path="/admin/forms" element={<Guard perm="admin.forms.manage"><AdminForms /></Guard>} />
      <Route path="/admin/settings" element={<Guard perm="admin.settings.manage"><AdminSettings /></Guard>} />
      <Route path="/admin/audit" element={<Guard perm="admin.audit.view"><AdminAudit /></Guard>} />
      <Route path="/admin/sessions" element={<Guard perm="admin.sessions.manage"><AdminSessions /></Guard>} />
      <Route path="*" element={<Navigate to="/" replace />} />
    </Routes>
  )
}
