import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { createBrowserRouter, RouterProvider } from 'react-router-dom'
import './index.css'
import { App } from './App'
import { AuthProvider } from './auth/AuthProvider'
import { RequireAuth } from './auth/RequireAuth'
import { ApprovalsPage } from './pages/ApprovalsPage'
import { DashboardPage } from './pages/DashboardPage'
import { LaunchPage } from './pages/LaunchPage'
import { LoginPage } from './pages/LoginPage'
import { ProjectsHomePage } from './pages/ProjectsHomePage'
import { RunsPage } from './pages/RunsPage'
import { SettingsPage } from './pages/SettingsPage'
import { SkillsPage } from './pages/SkillsPage'
import { TaskPage } from './pages/TaskPage'
import { TeamEditorPage } from './pages/TeamEditorPage'
import { TreePage } from './pages/TreePage'
import { applyTheme, preferredTheme } from './lib/theme'

// Before first paint, so light-mode users never see a flash of the dark shell.
applyTheme(preferredTheme())

const router = createBrowserRouter([
  { path: '/login', element: <LoginPage /> },
  // Home: the project picker (the "normal" navbar — no in-project nav).
  {
    path: '/',
    element: (
      <RequireAuth>
        <ProjectsHomePage />
      </RequireAuth>
    ),
  },
  // In-project shell: everything else lives under a project.
  {
    path: '/project/:projectId',
    element: (
      <RequireAuth>
        <App />
      </RequireAuth>
    ),
    children: [
      { index: true, element: <DashboardPage /> },
      { path: 'runs', element: <RunsPage /> },
      { path: 'runs/:date', element: <RunsPage /> },
      { path: 'tasks/:taskId', element: <TaskPage /> },
      { path: 'launch', element: <LaunchPage /> },
      { path: 'agents', element: <TreePage /> },
      { path: 'teams/new', element: <TeamEditorPage /> },
      { path: 'teams/:teamId', element: <TeamEditorPage /> },
      { path: 'approved', element: <ApprovalsPage /> },
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
