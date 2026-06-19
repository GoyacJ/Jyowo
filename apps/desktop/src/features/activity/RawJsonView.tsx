export type RawJsonDetails = {
  payload: Record<string, unknown>
  withheld?: boolean
}

const maxRenderedPayloadLength = 4000
const maxRenderedPayloadLines = 160

export function RawJsonView({ rawJson }: { rawJson: RawJsonDetails }) {
  if (rawJson.withheld) {
    return (
      <section aria-labelledby="raw-json-title" className="rounded-md border border-border p-4">
        <h3 className="font-medium" id="raw-json-title">
          Raw JSON
        </h3>
        <p className="mt-3 text-muted-foreground text-sm">Raw JSON withheld by policy</p>
      </section>
    )
  }

  const payloadPreview = createJsonPreview(rawJson.payload, maxRenderedPayloadLength)

  return (
    <section aria-labelledby="raw-json-title" className="rounded-md border border-border p-4">
      <div className="flex items-center justify-between gap-3">
        <h3 className="font-medium" id="raw-json-title">
          Raw JSON
        </h3>
        {payloadPreview.truncated ? (
          <span className="text-muted-foreground text-xs">Payload truncated</span>
        ) : null}
      </div>
      <pre className="mt-4 overflow-x-auto rounded-md bg-muted p-3 text-xs">
        <code>
          {payloadPreview.lines.map((line, index) => (
            <span className="block" key={`${index}-${line}`}>
              {line}
            </span>
          ))}
        </code>
      </pre>
    </section>
  )
}

type JsonPreviewState = {
  length: number
  lines: string[]
  truncated: boolean
}

function createJsonPreview(payload: Record<string, unknown>, maxLength: number): JsonPreviewState {
  const state: JsonPreviewState = {
    length: 0,
    lines: [],
    truncated: false,
  }
  appendJsonValue(payload, 0, state, maxLength, new Set())

  return state
}

function appendLine(line: string, state: JsonPreviewState, maxLength: number) {
  if (state.truncated || state.lines.length >= maxRenderedPayloadLines) {
    state.truncated = true
    return
  }

  if (state.length + line.length > maxLength) {
    const remaining = Math.max(0, maxLength - state.length)
    if (remaining > 0) {
      state.lines.push(`${line.slice(0, remaining)}...`)
    }
    state.truncated = true
    return
  }

  state.lines.push(line)
  state.length += line.length + 1
}

function appendJsonValue(
  value: unknown,
  depth: number,
  state: JsonPreviewState,
  maxLength: number,
  seen: Set<unknown>,
) {
  if (state.truncated) {
    return
  }

  if (typeof value === 'string') {
    const rendered = quoteJsonStringPreview(value, maxLength - state.length)
    appendLine(`${indent(depth)}${rendered.text}`, state, maxLength)
    state.truncated = state.truncated || rendered.truncated
    return
  }

  if (typeof value === 'number' || typeof value === 'boolean' || value === null) {
    appendLine(`${indent(depth)}${String(value)}`, state, maxLength)
    return
  }

  if (Array.isArray(value)) {
    appendArrayPreview(value, depth, state, maxLength, seen)
    return
  }

  if (typeof value === 'object') {
    appendObjectPreview(value as Record<string, unknown>, depth, state, maxLength, seen)
    return
  }

  appendLine(`${indent(depth)}null`, state, maxLength)
}

function appendObjectPreview(
  value: Record<string, unknown>,
  depth: number,
  state: JsonPreviewState,
  maxLength: number,
  seen: Set<unknown>,
) {
  if (seen.has(value)) {
    appendLine(`${indent(depth)}"[Circular]"`, state, maxLength)
    return
  }

  seen.add(value)
  appendLine(`${indent(depth)}{`, state, maxLength)

  let renderedEntries = 0
  for (const key in value) {
    if (!Object.hasOwn(value, key)) {
      continue
    }

    if (state.truncated) {
      break
    }

    const childValue = value[key]
    const propertyIndent = indent(depth + 1)
    const renderedKey = quoteJsonStringPreview(
      key,
      maxLength - state.length - propertyIndent.length - 2,
    )
    const prefix = `${propertyIndent}${renderedKey.text}: `
    appendJsonProperty(prefix, childValue, depth + 1, state, maxLength, seen)
    state.truncated = state.truncated || renderedKey.truncated
    renderedEntries += 1
  }

  if (renderedEntries === 0 && state.truncated) {
    seen.delete(value)
    return
  }

  appendLine(`${indent(depth)}}`, state, maxLength)
  seen.delete(value)
}

function appendArrayPreview(
  value: unknown[],
  depth: number,
  state: JsonPreviewState,
  maxLength: number,
  seen: Set<unknown>,
) {
  if (seen.has(value)) {
    appendLine(`${indent(depth)}"[Circular]"`, state, maxLength)
    return
  }

  seen.add(value)
  appendLine(`${indent(depth)}[`, state, maxLength)

  for (const childValue of value) {
    if (state.truncated) {
      break
    }

    appendJsonValue(childValue, depth + 1, state, maxLength, seen)
  }

  appendLine(`${indent(depth)}]`, state, maxLength)
  seen.delete(value)
}

function appendJsonProperty(
  prefix: string,
  value: unknown,
  depth: number,
  state: JsonPreviewState,
  maxLength: number,
  seen: Set<unknown>,
) {
  if (
    typeof value === 'string' ||
    typeof value === 'number' ||
    typeof value === 'boolean' ||
    value === null
  ) {
    const renderedValue =
      typeof value === 'string'
        ? quoteJsonStringPreview(value, maxLength - state.length - prefix.length)
        : { text: String(value), truncated: false }
    appendLine(`${prefix}${renderedValue.text}`, state, maxLength)
    state.truncated = state.truncated || renderedValue.truncated
    return
  }

  appendLine(prefix.trimEnd(), state, maxLength)
  appendJsonValue(value, depth + 1, state, maxLength, seen)
}

function quoteJsonStringPreview(
  value: string,
  remainingBudget: number,
): {
  text: string
  truncated: boolean
} {
  const maxRawLength = Math.max(0, remainingBudget - 5)

  if (value.length > maxRawLength) {
    return {
      text: `"${escapeJsonString(value.slice(0, maxRawLength))}..."`,
      truncated: true,
    }
  }

  return {
    text: `"${escapeJsonString(value)}"`,
    truncated: false,
  }
}

function escapeJsonString(value: string): string {
  return value
    .replaceAll('\\', '\\\\')
    .replaceAll('"', '\\"')
    .replaceAll('\n', '\\n')
    .replaceAll('\r', '\\r')
    .replaceAll('\t', '\\t')
}

function indent(depth: number): string {
  return '  '.repeat(depth)
}
