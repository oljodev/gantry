import { describe, expect, it } from 'vitest'
import { parseUnifiedDiff } from './diff'

const SAMPLE = `diff --git a/hello.txt b/hello.txt
new file mode 100644
index 0000000..3b18e51
--- /dev/null
+++ b/hello.txt
@@ -0,0 +1 @@
+hello world
diff --git a/src/mod.py b/src/mod.py
index 1111111..2222222 100644
--- a/src/mod.py
+++ b/src/mod.py
@@ -1,3 +1,3 @@
 def greet():
-    return "hi"
+    return "hello"
`

describe('parseUnifiedDiff', () => {
  it('splits files and counts additions/deletions', () => {
    const files = parseUnifiedDiff(SAMPLE)
    expect(files.map((f) => f.path)).toEqual(['hello.txt', 'src/mod.py'])

    expect(files[0].isNew).toBe(true)
    expect(files[0].additions).toBe(1)
    expect(files[0].deletions).toBe(0)

    expect(files[1].additions).toBe(1)
    expect(files[1].deletions).toBe(1)
    expect(files[1].lines.map((l) => l.kind)).toEqual(['hunk', 'context', 'del', 'add'])
    expect(files[1].lines[3].text).toBe('    return "hello"')
  })

  it('never mistakes header +++/--- lines for hunk content', () => {
    const files = parseUnifiedDiff(SAMPLE)
    const texts = files[0].lines.map((l) => l.text)
    expect(texts).not.toContain('++ b/hello.txt')
    expect(files[0].lines.map((l) => l.kind)).toEqual(['hunk', 'add'])
  })

  it('handles empty input', () => {
    expect(parseUnifiedDiff('')).toEqual([])
  })
})
