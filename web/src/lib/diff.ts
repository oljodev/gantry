// Minimal unified-diff parser for the diff viewer. Input is the exact
// output of `git diff --cached` captured in `diff` events.

export interface DiffLine {
  kind: 'add' | 'del' | 'context' | 'hunk'
  text: string
}

export interface FileDiff {
  path: string
  oldPath: string
  isNew: boolean
  isDeleted: boolean
  lines: DiffLine[]
  additions: number
  deletions: number
}

export function parseUnifiedDiff(diff: string): FileDiff[] {
  const files: FileDiff[] = []
  let current: FileDiff | null = null
  let inHunks = false

  for (const line of diff.split('\n')) {
    if (line.startsWith('diff --git ')) {
      // "diff --git a/path b/path" — take the b/ side as the display path.
      const match = /^diff --git a\/(.*) b\/(.*)$/.exec(line)
      current = {
        path: match?.[2] ?? line.slice('diff --git '.length),
        oldPath: match?.[1] ?? '',
        isNew: false,
        isDeleted: false,
        lines: [],
        additions: 0,
        deletions: 0,
      }
      files.push(current)
      inHunks = false
      continue
    }
    if (!current) continue
    if (line.startsWith('new file mode')) current.isNew = true
    if (line.startsWith('deleted file mode')) current.isDeleted = true
    if (line.startsWith('@@')) {
      inHunks = true
      current.lines.push({ kind: 'hunk', text: line })
      continue
    }
    if (!inHunks) continue // still in the header (index/mode lines)
    if (line.startsWith('+')) {
      current.lines.push({ kind: 'add', text: line.slice(1) })
      current.additions++
    } else if (line.startsWith('-')) {
      current.lines.push({ kind: 'del', text: line.slice(1) })
      current.deletions++
    } else if (line.startsWith(' ')) {
      current.lines.push({ kind: 'context', text: line.slice(1) })
    }
    // Empty-string split artifacts, "\ No newline at end of file" and
    // similar markers are dropped: empty context lines arrive as " ".
  }
  return files
}
