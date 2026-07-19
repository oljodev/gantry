export function shortId(id: string): string {
  return id.slice(0, 8)
}

export function clockTime(iso: string): string {
  return new Date(iso).toLocaleTimeString(undefined, { hour12: false })
}

export function relativeTime(iso: string): string {
  const seconds = Math.max(0, (Date.now() - new Date(iso).getTime()) / 1000)
  if (seconds < 60) return `${Math.floor(seconds)}s ago`
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m ago`
  if (seconds < 86_400) return `${Math.floor(seconds / 3600)}h ago`
  return `${Math.floor(seconds / 86_400)}d ago`
}

export function compactJson(value: unknown, max = 160): string {
  const text = JSON.stringify(value)
  if (text === undefined) return ''
  return text.length > max ? `${text.slice(0, max)}…` : text
}
