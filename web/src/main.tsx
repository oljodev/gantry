import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { createBrowserRouter, RouterProvider } from 'react-router-dom'
import './index.css'
import { App } from './App'
import { AuthProvider } from './auth/AuthProvider'
import { RequireAuth } from './auth/RequireAuth'
import { AgentsPage } from './pages/AgentsPage'
import { ApprovalsPage } from './pages/ApprovalsPage'
import { DashboardPage } from './pages/DashboardPage'
import { LaunchPage } from './pages/LaunchPage'
import { LoginPage } from './pages/LoginPage'
import { RunsPage } from './pages/RunsPage'
import { SettingsPage } from './pages/SettingsPage'
import { SkillsPage } from './pages/SkillsPage'
import { TaskPage } from './pages/TaskPage'
import { TeamEditorPage } from './pages/TeamEditorPage'
import { TeamsPage } from './pages/TeamsPage'
import { applyTheme, preferredTheme } from './lib/theme'

// Before first paint, so light-mode users never see a flash of the dark shell.
applyTheme(preferredTheme())

const router = createBrowserRouter([
  { path: '/login', element: <LoginPage /> },
  {
    path: '/',
    element: (
      <RequireAuth>
        <App />
      </RequireAuth>
    ),
    children: [
      { index: true, element: <DashboardPage /> },
      { path: 'runs', element: <RunsPage /> },
      { path: 'tasks/:taskId', element: <TaskPage /> },
      { path: 'launch', element: <LaunchPage /> },
      { path: 'agents', element: <AgentsPage /> },
      { path: 'teams', element: <TeamsPage /> },
      { path: 'teams/new', element: <TeamEditorPage /> },
      { path: 'teams/:teamId', element: <TeamEditorPage /> },
      { path: 'approvals', element: <ApprovalsPage /> },
      { path: 'skills', element: <SkillsPage /> },
      { path: 'settings', element: <SettingsPage /> },
    ],
  },
])

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <AuthProvider>
      <RouterProvider router={router} />
    </AuthProvider>
  </StrictMode>,
)
