import { spawnSync } from 'node:child_process'
import { createRequire } from 'node:module'
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const desktopRoot = join(repoRoot, 'apps', 'desktop')
const generatedRoot = join(desktopRoot, 'src', 'generated')
const schemaPath = join(generatedRoot, 'daemon-protocol.schema.json')
const typesPath = join(generatedRoot, 'daemon-protocol.ts')
const checkOnly = process.argv.includes('--check')

const cargo = spawnSync(
  'cargo',
  ['run', '-q', '-p', 'jyowo-harness-contracts', '--example', 'export_daemon_schema'],
  { cwd: repoRoot, encoding: 'utf8' },
)

if (cargo.error) {
  throw cargo.error
}
if (cargo.status !== 0) {
  process.stderr.write(cargo.stderr)
  process.exit(cargo.status ?? 1)
}

const rustSchema = JSON.parse(cargo.stdout)
const schema = normalizeTaggedVariants(rustSchema)
const schemaText = `${JSON.stringify(schema, null, 2)}\n`
const desktopRequire = createRequire(join(desktopRoot, 'package.json'))
const compilerPath = desktopRequire.resolve('json-schema-to-typescript')
const { compile } = await import(pathToFileURL(compilerPath).href)
const typesText = await compile(schema, 'DaemonProtocol', {
  additionalProperties: false,
  bannerComment:
    '/* eslint-disable */\n// Generated from jyowo-harness-contracts. Do not edit by hand.',
  cwd: generatedRoot,
  style: {
    printWidth: 100,
    semi: false,
    singleQuote: true,
    tabWidth: 2,
    trailingComma: 'all',
  },
  unknownAny: true,
  unreachableDefinitions: true,
})

const outputs = [
  [schemaPath, schemaText],
  [typesPath, typesText],
]

if (checkOnly) {
  const stale = outputs
    .filter(([path, content]) => !existsSync(path) || readFileSync(path, 'utf8') !== content)
    .map(([path]) => path)
  if (stale.length > 0) {
    console.error(`Generated daemon protocol is stale:\n${stale.join('\n')}`)
    process.exit(1)
  }
  console.log('Generated daemon protocol is current.')
  process.exit(0)
}

mkdirSync(generatedRoot, { recursive: true })
for (const [path, content] of outputs) {
  if (!existsSync(path) || readFileSync(path, 'utf8') !== content) {
    writeFileSync(path, content)
  }
}
console.log('Generated daemon protocol schema and TypeScript types.')

function normalizeTaggedVariants(root) {
  const definitions = root.$defs ?? {}

  function visit(value) {
    if (Array.isArray(value)) {
      return value.map(visit)
    }
    if (value === null || typeof value !== 'object') {
      return value
    }

    const referenceName = localDefinitionName(value.$ref)
    if (referenceName && Object.keys(value).length > 1) {
      const referenced = definitions[referenceName]
      if (referenced?.type === 'object' && value.type === 'object') {
        const { $ref: _reference, properties = {}, required = [], ...siblings } = value
        return visit({
          ...referenced,
          ...siblings,
          properties: {
            ...(referenced.properties ?? {}),
            ...properties,
          },
          required: [...new Set([...(referenced.required ?? []), ...required])],
        })
      }
    }

    return Object.fromEntries(Object.entries(value).map(([key, child]) => [key, visit(child)]))
  }

  return visit(root)
}

function localDefinitionName(reference) {
  const prefix = '#/$defs/'
  return typeof reference === 'string' && reference.startsWith(prefix)
    ? reference.slice(prefix.length)
    : undefined
}
