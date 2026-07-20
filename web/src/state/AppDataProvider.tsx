// One firehose connection for the whole app shell. Every page shares it:
// the activity feed gets every event, the sidebar badge gets approvals, and
// data views re-fetch on a 300ms debounce after state-changing events.

import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from 'react'
import { getStats, listApprovals, type ApprovalItem } from '../api/client'
import { openFirehose, type ConnectionState } from '../api/stream'
import type { Stats, TaskEvent } from '../api/types'

interface AppDataValue {
  feed: TaskEvent[]
  stats: Stats | null
  approvals: ApprovalItem[]
  connection: ConnectionState
  /** Monotonic counter bumped after every (debounced) data-changing event —
   *  depend on it in useEffect to refetch page-local data. */
  version: number
  refetch: () => void
}

const AppDataContext = createContext<AppDataValue>({
  feed: [],
  stats: null,
  approvals: [],
  connection: 'connecting',
  version: 0,
  refetch: () => {},
})

// eslint-disable-next-line react-refresh/only-export-components
export function useAppData(): AppDataValue {
  return useContext(AppDataContext)
}

export function AppDataProvider({ children }: { children: ReactNode }) {
  const [feed, setFeed] = useState<TaskEvent[]>([])
  const [stats, setStats] = useState<Stats | null>(null)
  const [approvals, setApprovals] = useState<ApprovalItem[]>([])
  const [connection, setConnection] = useState<ConnectionState>('connecting')
  const [version, setVersion] = useState(0)
  const debounce = useRef<ReturnType<typeof setTimeout> | null>(null)

  const refetch = useCallback(() => {
    getStats().then(setStats).catch(console.error)
    listApprovals().then(setApprovals).catch(console.error)
    setVersion((v) => v + 1)
  }, [])

  useEffect(() => {
    refetch()
    const stop = openFirehose((event) => {
      setFeed((prev) => [...prev.slice(-199), event])
      if (!event.event_type.startsWith('task_') && !event.event_type.startsWith('approval_'))
        return
      if (debounce.current !== null) return
      debounce.current = setTimeout(() => {
        debounce.current = null
        refetch()
      }, 300)
    }, setConnection)
    return () => {
      stop()
      if (debounce.current !== null) clearTimeout(debounce.current)
    }
  }, [refetch])

  return (
    <AppDataContext.Provider value={{ feed, stats, approvals, connection, version, refetch }}>
      {children}
    </AppDataContext.Provider>
  )
}
