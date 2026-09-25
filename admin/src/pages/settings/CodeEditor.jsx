import { useMemo } from 'react'
import CodeMirror from '@uiw/react-codemirror'
import { yaml } from '@codemirror/lang-yaml'
import { json } from '@codemirror/lang-json'

export default function CodeEditor({ value, onChange, format, theme, label }) {
  const extensions = useMemo(() => [format === 'json' ? json() : yaml()], [format])
  return (
    <div className="overflow-hidden rounded-xl border border-border text-[13px]">
      <CodeMirror
        value={value}
        onChange={onChange}
        extensions={extensions}
        theme={theme === 'light' ? 'light' : 'dark'}
        height="65vh"
        basicSetup={{ foldGutter: true, highlightActiveLine: true, tabSize: 2 }}
        aria-label={label}
      />
    </div>
  )
}
