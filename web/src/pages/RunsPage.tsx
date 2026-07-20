import { useEffect } from 'react'
import { RunsTable } from '../components/RunsTable'

export function RunsPage() {
  useEffect(() => {
    document.title = 'Gantry — runs'
  }, [])
  return (
    <div className="flex flex-col gap-4">
      <h1 className="text-lg font-semibold tracking-tight">Runs</h1>
      <RunsTable />
    </div>
  )
}
