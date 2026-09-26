import { lazy, Suspense, useState } from 'react'
import { createBrowserRouter, Navigate, RouterProvider } from 'react-router'
import { AuthProvider, useAuth } from './components/Auth.jsx'
import { ToastProvider } from './components/Toast.jsx'
import { ConfirmProvider } from './components/Confirm.jsx'
import { ResultProvider } from './components/ResultDrawer.jsx'
import { Layout } from './components/Layout.jsx'
import { Spinner } from './components/ui.jsx'
import { LoginPage } from './pages/Login.jsx'
import { OverviewPage } from './pages/Overview.jsx'
import { TrackersPage } from './pages/Trackers.jsx'
import { JobsPage } from './pages/Jobs.jsx'
import { CloudflarePage } from './pages/Cloudflare.jsx'
import { MaintenancePage } from './pages/Maintenance.jsx'
import { LogsPage } from './pages/Logs.jsx'
import { WafPage } from './pages/waf/Waf.jsx'
import { WafOverview } from './pages/waf/WafOverview.jsx'
import { WafLog } from './pages/waf/WafLog.jsx'
import { WafIps } from './pages/waf/WafIps.jsx'
import { WafRules } from './pages/waf/WafRules.jsx'
import { getBase } from './lib/base.js'

// The settings editor pulls in CodeMirror - load it on demand.
const SettingsPage = lazy(() => import('./pages/settings/Settings.jsx'))

function Gate() {
  const { status, error, refresh } = useAuth()
  if (status === 'loading') {
    return (
      <div className="grid min-h-screen place-items-center">
        <Spinner className="size-6" label="Загрузка…" />
      </div>
    )
  }
  if (status === 'error') {
    return (
      <div className="grid min-h-screen place-items-center p-4">
        <div className="card max-w-md p-6 text-center">
          <p className="font-medium">Не удалось связаться с сервером</p>
          <p className="mt-2 text-sm text-muted">{error?.message}</p>
          <button type="button" className="btn btn-primary mt-4" onClick={refresh}>
            Повторить
          </button>
        </div>
      </div>
    )
  }
  if (status !== 'authed') return <LoginPage />
  return <Layout />
}

export function createAppRouter(basename = getBase()) {
  return createBrowserRouter(
    [
      {
        path: '/',
        element: <Gate />,
        children: [
          { index: true, element: <OverviewPage /> },
          { path: 'trackers', element: <TrackersPage /> },
          { path: 'jobs', element: <JobsPage /> },
          { path: 'cloudflare', element: <CloudflarePage /> },
          {
            path: 'settings',
            element: (
              <Suspense fallback={<Spinner className="size-5" label="Загрузка редактора…" />}>
                <SettingsPage />
              </Suspense>
            ),
          },
          { path: 'maintenance', element: <MaintenancePage /> },
          { path: 'logs', element: <LogsPage /> },
          {
            path: 'waf',
            element: <WafPage />,
            children: [
              { index: true, element: <WafOverview /> },
              { path: 'log', element: <WafLog /> },
              { path: 'ips', element: <WafIps /> },
              { path: 'rules', element: <WafRules /> },
              { path: '*', element: <Navigate to="/waf" replace /> },
            ],
          },
          { path: '*', element: <Navigate to="/" replace /> },
        ],
      },
    ],
    { basename },
  )
}

export default function App() {
  const [router] = useState(() => createAppRouter())
  return (
    <ToastProvider>
      <ConfirmProvider>
        <ResultProvider>
          <AuthProvider>
            <RouterProvider router={router} />
          </AuthProvider>
        </ResultProvider>
      </ConfirmProvider>
    </ToastProvider>
  )
}
