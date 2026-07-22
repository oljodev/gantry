// Project-scoped routing helpers. Every in-project page lives under
// /project/:projectId, so links are built from the current project id.

import { useParams } from 'react-router-dom'

export function useProjectId(): string {
  const { projectId } = useParams<{ projectId: string }>()
  return projectId ?? ''
}

/** Absolute path to a sub-route of a project, e.g. projectPath(id, 'runs'). */
export function projectPath(projectId: string, sub = ''): string {
  return `/project/${projectId}${sub ? `/${sub}` : ''}`
}
