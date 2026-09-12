# What a React artifact may import

Six modules, bundled into the sandbox. Nothing else resolves: an import of anything not on this
list fails at compile time with an unresolved-module error, before the component runs.

This list is kept in step with `desktop/artifact-runtime/src/react/modules.ts`; adding a module
means editing that file, the allowlist sentence in `desktop/assets/prompts/core.md`, and this
reference.

| Module | What it gives you |
|---|---|
| `react` | the library itself: hooks, `Fragment`, `memo`, `createContext` |
| `react-dom` | the DOM package; you rarely need it directly |
| `react-dom/client` | `createRoot`, if you want to mount something yourself — the runtime already mounts your default export |
| `react/jsx-runtime` | the automatic JSX transform; imported for you, never written by hand |
| `lucide-react` | icons, as components: `import { ArrowRight } from 'lucide-react'` |
| `recharts` | charts: `LineChart`, `BarChart`, `AreaChart`, `PieChart`, `XAxis`, `YAxis`, `Tooltip`, `ResponsiveContainer` |
| `clsx` | conditional class names: `clsx('btn', active && 'btn-on')` |

## Notes that save a render

**Recharts needs a height.** `ResponsiveContainer` measures its parent, and a parent with no
height measures zero, which draws nothing at all. Give the wrapper an explicit height:

```jsx
<div className="h-64 w-full">
  <ResponsiveContainer>
    <LineChart data={data}>…</LineChart>
  </ResponsiveContainer>
</div>
```

**Lucide icon names are PascalCase** and come from the icon's own name: `ChevronDown`,
`TriangleAlert`, `CircleCheck`. An import that does not exist is `undefined` at render time
rather than an import error, so it shows as "type is invalid" — check the spelling first.

**There is no CSS import.** Tailwind utility classes are available; a `import './styles.css'`
does not resolve. For anything utilities cannot express, use a `style` object.

**No date or number formatting library.** `Intl.DateTimeFormat` and `Intl.NumberFormat` are in
the sandbox and cover most of what a formatting dependency would.
