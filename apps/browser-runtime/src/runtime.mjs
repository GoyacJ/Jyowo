import { spawn } from 'node:child_process'
import { createRequire } from 'node:module'
import { createInterface } from 'node:readline'
import { access, mkdir, readFile, realpath, rm, stat } from 'node:fs/promises'
import { constants } from 'node:fs'
import { isAbsolute, join, relative, resolve } from 'node:path'
import process from 'node:process'

const require = createRequire(import.meta.url)
const playwrightCli = require.resolve('@playwright/cli/playwright-cli.js')
const chromeDevToolsCli = require.resolve(
  'chrome-devtools-mcp/build/src/bin/chrome-devtools.js',
)
const sessionRoot = process.env.JYOWO_BROWSER_SESSION_ROOT
if (!sessionRoot) throw new Error('JYOWO_BROWSER_SESSION_ROOT is required')

const sessions = new Map()
let requestQueue = Promise.resolve()
let shuttingDown = false

const input = createInterface({ input: process.stdin, crlfDelay: Infinity })
input.on('line', (line) => {
  requestQueue = requestQueue.then(() => handleLine(line)).catch(writeFatalError)
})
input.on('close', () => void shutdown())

process.on('SIGINT', () => void shutdown())
process.on('SIGTERM', () => void shutdown())
process.on('SIGHUP', () => void shutdown())

async function handleLine(line) {
  let request
  try {
    request = JSON.parse(line)
    validateRequest(request)
    const result = await dispatch(request)
    writeResponse({ id: request.id, ok: true, result })
  } catch (error) {
    writeResponse({
      id: Number.isSafeInteger(request?.id) ? request.id : 0,
      ok: false,
      error: error instanceof Error ? error.message : String(error),
    })
  }
}

function validateRequest(request) {
  if (!request || typeof request !== 'object') throw new Error('request must be an object')
  if (!Number.isSafeInteger(request.id) || request.id < 1) throw new Error('request ID is invalid')
  if (typeof request.method !== 'string') throw new Error('request method is invalid')
  if (request.method !== 'shutdown' && !/^[0-9A-HJKMNP-TV-Z]{26}$/.test(request.taskId)) {
    throw new Error('task ID is invalid')
  }
}

async function dispatch(request) {
  if (request.method === 'shutdown') {
    await closeAllSessions()
    queueMicrotask(() => process.exit(0))
    return { stopped: true }
  }
  if (request.method === 'status') return sessionState(request.taskId)
  if (request.method === 'close') {
    await closeSession(request.taskId)
    return stoppedState(request.taskId)
  }
  if (request.method === 'open' || request.method === 'show') {
    const session = await ensureSession(request.taskId, request.params?.url)
    return await readyState(session)
  }
  if (request.method === 'tool') return await executeTool(request.taskId, request.params)
  throw new Error(`unsupported browser host method: ${request.method}`)
}

async function ensureSession(taskId, initialUrl) {
  const existing = sessions.get(taskId)
  if (existing && existing.chrome.exitCode === null && existing.dashboard.exitCode === null) {
    if (initialUrl) await navigateSession(existing, normalizeUrl(initialUrl))
    return existing
  }
  if (existing) await closeSession(taskId)

  const root = join(sessionRoot, taskId)
  const profile = join(root, 'profile')
  const output = join(root, 'output')
  await mkdir(profile, { recursive: true, mode: 0o700 })
  await mkdir(output, { recursive: true, mode: 0o700 })
  await removeStaleChromeLocks(profile)

  const executable = await findChromeExecutable()
  const chrome = spawn(
    executable,
    [
      '--headless=new',
      '--remote-debugging-address=127.0.0.1',
      '--remote-debugging-port=0',
      `--user-data-dir=${profile}`,
      '--no-first-run',
      '--no-default-browser-check',
      '--disable-background-networking',
      '--disable-component-update',
      '--remote-allow-origins=*',
      'about:blank',
    ],
    { stdio: 'ignore' },
  )
  const endpoint = await waitForDevToolsEndpoint(profile, chrome)
  const session = {
    taskId,
    name: `jyowo-${taskId.toLowerCase()}`,
    root,
    output,
    endpoint,
    chrome,
    dashboard: null,
    dashboardUrl: null,
  }

  try {
    await runNode(
      [playwrightCli, `-s=${session.name}`, 'attach', `--cdp=${endpoint}`, '--json'],
      sessionEnvironment(session),
      { cwd: session.root },
    )
    await runNode(
      [
        chromeDevToolsCli,
        'start',
        `--sessionId=${session.name}`,
        `--browserUrl=${endpoint}`,
        '--no-usage-statistics',
        '--no-performance-crux',
        '--no-category-extensions',
      ],
      sessionEnvironment(session),
      { cwd: session.root },
    )
    const dashboard = await startDashboard(session)
    session.dashboard = dashboard.child
    session.dashboardUrl = dashboard.url
    sessions.set(taskId, session)
    if (initialUrl) await navigateSession(session, normalizeUrl(initialUrl))
    return session
  } catch (error) {
    await terminateProcess(chrome)
    await stopCliDaemons(session)
    throw error
  }
}

async function executeTool(taskId, params) {
  if (!params || typeof params !== 'object') throw new Error('tool parameters are required')
  const { toolName, input: toolInput } = params
  if (toolName === 'BrowserUse') {
    const input = await prepareBrowserUseInput(toolInput, params.workspaceRoot)
    const session = await ensureSession(taskId, input.action === 'open' ? input.url : null)
    if (input.action === 'open') return { session: await readyState(session), output: null }
    const { command, args } = browserUseCommand(input)
    const output = await runPlaywright(session, command, args)
    if (input.action === 'goto' || (input.action === 'tab_new' && input.url)) {
      await selectDevToolsPage(session, input.url)
    }
    return { session: await readyState(session), output }
  }
  if (toolName === 'BrowserDevTools') {
    const session = await ensureSession(taskId)
    const output = await runDevTools(session, toolInput)
    return { session: await readyState(session), output }
  }
  throw new Error(`unsupported built-in browser tool: ${toolName}`)
}

async function navigateSession(session, url) {
  await runPlaywright(session, 'goto', [url])
  await selectDevToolsPage(session, url)
}

async function selectDevToolsPage(session, url) {
  const listing = await runDevTools(session, { action: 'list_pages', args: {} })
  const pages = Array.isArray(listing?.pages) ? listing.pages : []
  const page = pages.find((candidate) => candidate.url === url)
  if (page && !page.selected) {
    await runDevTools(session, { action: 'select_page', args: { pageId: page.id } })
  }
}

async function prepareBrowserUseInput(input, workspaceRoot) {
  if (!input || typeof input !== 'object') throw new Error('BrowserUse input is required')
  const prepared = { ...input }
  if (['open', 'goto', 'tab_new'].includes(prepared.action) && prepared.url) {
    prepared.url = normalizeUrl(prepared.url)
  }
  if (prepared.action === 'upload') {
    prepared.path = await workspaceFile(required(prepared.path, 'path'), workspaceRoot)
  }
  return prepared
}

function browserUseCommand(input) {
  if (!input || typeof input !== 'object') throw new Error('BrowserUse input is required')
  const target = input.target ?? input.selector
  const commands = {
    goto: () => ['goto', required(input.url, 'url')],
    snapshot: () => ['snapshot', ...optional(target)],
    find: () => ['find', required(input.text, 'text')],
    click: () => ['click', required(target, 'target'), ...optional(input.button)],
    dblclick: () => ['dblclick', required(target, 'target'), ...optional(input.button)],
    fill: () => ['fill', required(target, 'target'), required(input.text, 'text')],
    type: () => ['type', required(input.text, 'text')],
    press: () => ['press', required(input.key, 'key')],
    hover: () => ['hover', required(target, 'target')],
    select: () => ['select', required(target, 'target'), required(input.value, 'value')],
    upload: () => ['upload', required(input.path, 'path')],
    screenshot: () => ['screenshot', ...optional(target)],
    pdf: () => ['pdf'],
    back: () => ['go-back'],
    forward: () => ['go-forward'],
    reload: () => ['reload'],
    tab_list: () => ['tab-list'],
    tab_new: () => ['tab-new', ...optional(input.url)],
    tab_close: () => ['tab-close', ...optional(input.index)],
    tab_select: () => ['tab-select', required(input.index, 'index')],
    mousemove: () => ['mousemove', required(input.x, 'x'), required(input.y, 'y')],
    mousedown: () => ['mousedown', ...optional(input.button)],
    mouseup: () => ['mouseup', ...optional(input.button)],
    mousewheel: () => [
      'mousewheel',
      required(input.delta_x, 'delta_x'),
      required(input.delta_y, 'delta_y'),
    ],
    console: () => ['console'],
    requests: () => ['requests'],
    request: () => ['request', required(input.index, 'index')],
    run_code: () => ['run-code', required(input.expression, 'expression')],
  }
  const createCommand = commands[input.action]
  if (!createCommand) throw new Error(`unsupported BrowserUse action: ${input.action}`)
  const selected = createCommand()
  return { command: selected[0], args: selected.slice(1).map(String) }
}

const devToolsPositionals = {
  select_page: ['pageId'],
  new_page: ['url'],
  close_page: ['pageId'],
  evaluate_script: ['function'],
  get_console_message: ['msgid'],
  performance_analyze_insight: ['insightSetId', 'insightName'],
}

async function runDevTools(session, input) {
  if (!input || typeof input !== 'object') throw new Error('BrowserDevTools input is required')
  const action = input.action
  if (typeof action !== 'string' || !/^[a-z][a-z0-9_]*$/.test(action)) {
    throw new Error('BrowserDevTools action is invalid')
  }
  const args = input.args && typeof input.args === 'object' ? { ...input.args } : {}
  if (action === 'new_page') args.url = normalizeUrl(required(args.url, 'url'))
  if (action === 'navigate_page' && args.url) args.url = normalizeUrl(args.url)
  const argv = [chromeDevToolsCli, action]
  for (const name of devToolsPositionals[action] ?? []) {
    argv.push(String(required(args[name], name)))
    delete args[name]
  }
  for (const [name, value] of Object.entries(args)) appendCliOption(argv, name, value)
  argv.push(`--sessionId=${session.name}`, '--output-format=json')
  return parseCliOutput(
    await runNode(argv, sessionEnvironment(session), { cwd: session.root }),
  )
}

async function runPlaywright(session, command, args) {
  const output = parseCliOutput(
    await runNode(
      [playwrightCli, `-s=${session.name}`, command, ...args, '--json'],
      sessionEnvironment(session),
      { cwd: session.root },
    ),
  )
  if (output && typeof output === 'object' && output.isError === true) {
    throw new Error(typeof output.error === 'string' ? output.error : 'Playwright command failed')
  }
  return output
}

function sessionEnvironment(session) {
  return {
    ...process.env,
    CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS: '1',
    PLAYWRIGHT_MCP_OUTPUT_DIR: session.output,
  }
}

async function startDashboard(session) {
  const child = spawn(
    process.execPath,
    [playwrightCli, `-s=${session.name}`, 'show', '--port=0', '--host=127.0.0.1'],
    {
      cwd: session.root,
      env: sessionEnvironment(session),
      stdio: ['ignore', 'pipe', 'pipe'],
    },
  )
  return await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      void terminateProcess(child)
      reject(new Error('Playwright Dashboard did not start within 15 seconds'))
    }, 15_000)
    const inspect = (data) => {
      const match = String(data).match(/Listening on (http:\/\/127\.0\.0\.1:\d+)/)
      if (!match) return
      clearTimeout(timer)
      resolve({ child, url: match[1] })
    }
    child.stdout.on('data', inspect)
    child.stderr.on('data', inspect)
    child.once('exit', (code) => {
      clearTimeout(timer)
      reject(new Error(`Playwright Dashboard stopped during startup (${code ?? 'signal'})`))
    })
    child.once('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
  })
}

async function readyState(session) {
  const page = await currentPage(session.endpoint)
  return {
    taskId: session.taskId,
    status: 'ready',
    dashboardUrl: session.dashboardUrl,
    currentUrl: page?.url ?? null,
    title: page?.title ?? null,
    unavailableReason: null,
  }
}

async function sessionState(taskId) {
  const session = sessions.get(taskId)
  if (!session) return stoppedState(taskId)
  if (session.chrome.exitCode !== null || session.dashboard.exitCode !== null) {
    await closeSession(taskId)
    return {
      ...stoppedState(taskId),
      status: 'failed',
      unavailableReason: 'browser process stopped unexpectedly',
    }
  }
  return await readyState(session)
}

function stoppedState(taskId) {
  return {
    taskId,
    status: 'stopped',
    dashboardUrl: null,
    currentUrl: null,
    title: null,
    unavailableReason: null,
  }
}

async function closeSession(taskId) {
  const session = sessions.get(taskId)
  if (!session) return
  sessions.delete(taskId)
  await terminateProcess(session.dashboard)
  await stopCliDaemons(session)
  await terminateProcess(session.chrome)
}

async function stopCliDaemons(session) {
  await runNode(
    [chromeDevToolsCli, 'stop', `--sessionId=${session.name}`],
    sessionEnvironment(session),
    { allowFailure: true, cwd: session.root },
  )
  await runNode(
    [playwrightCli, `-s=${session.name}`, 'detach', '--json'],
    sessionEnvironment(session),
    { allowFailure: true, cwd: session.root },
  )
}

async function closeAllSessions() {
  await Promise.all([...sessions.keys()].map(closeSession))
}

async function shutdown() {
  if (shuttingDown) return
  shuttingDown = true
  input.close()
  await closeAllSessions()
  process.exit(0)
}

async function waitForDevToolsEndpoint(profile, chrome) {
  const portFile = join(profile, 'DevToolsActivePort')
  const deadline = Date.now() + 15_000
  while (Date.now() < deadline) {
    if (chrome.exitCode !== null) throw new Error(`Chrome stopped during startup (${chrome.exitCode})`)
    try {
      const [port] = (await readFile(portFile, 'utf8')).trim().split(/\r?\n/)
      if (/^\d+$/.test(port)) return `http://127.0.0.1:${port}`
    } catch {
      // Chrome creates the file after its remote debugging server is ready.
    }
    await delay(100)
  }
  throw new Error('Chrome DevTools endpoint did not start within 15 seconds')
}

async function currentPage(endpoint) {
  try {
    const response = await fetch(`${endpoint}/json/list`, { signal: AbortSignal.timeout(2_000) })
    if (!response.ok) return null
    const targets = await response.json()
    const pages = targets.filter((target) => target.type === 'page')
    return pages.find((target) => target.url !== 'about:blank') ?? pages[0] ?? null
  } catch {
    return null
  }
}

async function findChromeExecutable() {
  const configured = process.env.JYOWO_BROWSER_EXECUTABLE
  if (configured) {
    await access(configured, constants.X_OK)
    return configured
  }
  const candidates =
    process.platform === 'darwin'
      ? [
          '/Applications/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing',
          '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
        ]
      : process.platform === 'win32'
        ? [
            join(process.env.PROGRAMFILES ?? '', 'Google', 'Chrome', 'Application', 'chrome.exe'),
            join(process.env['PROGRAMFILES(X86)'] ?? '', 'Google', 'Chrome', 'Application', 'chrome.exe'),
          ]
        : ['/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium']
  for (const candidate of candidates) {
    try {
      await access(candidate, constants.X_OK)
      return candidate
    } catch {
      // Continue through development-only system Chrome candidates.
    }
  }
  throw new Error('Chrome for Testing is not installed in the Jyowo browser runtime')
}

async function removeStaleChromeLocks(profile) {
  for (const name of ['DevToolsActivePort', 'SingletonCookie', 'SingletonLock', 'SingletonSocket']) {
    await rm(join(profile, name), { force: true, recursive: true })
  }
}

function normalizeUrl(value) {
  if (typeof value !== 'string' || value.trim() === '') throw new Error('url is required')
  const trimmed = value.trim()
  const scheme = trimmed.match(/^([a-z][a-z0-9+.-]*):/i)?.[1]?.toLowerCase()
  if (scheme && !['http', 'https'].includes(scheme)) {
    throw new Error('browser navigation only supports http and https URLs')
  }
  const localAddress = /^(?:localhost|127\.0\.0\.1|\[::1\])(?::\d+)?(?:[/?#]|$)/i.test(trimmed)
  const normalized = scheme ? trimmed : `${localAddress ? 'http' : 'https'}://${trimmed}`
  let url
  try {
    url = new URL(normalized)
  } catch {
    throw new Error('url is invalid')
  }
  if (!['http:', 'https:'].includes(url.protocol)) {
    throw new Error('browser navigation only supports http and https URLs')
  }
  return url.toString()
}

async function workspaceFile(value, workspaceRoot) {
  if (typeof workspaceRoot !== 'string' || workspaceRoot === '') {
    throw new Error('workspace root is required for browser file uploads')
  }
  const root = await realpath(workspaceRoot)
  const file = await realpath(resolve(root, value))
  const workspaceRelative = relative(root, file)
  if (workspaceRelative.startsWith('..') || isAbsolute(workspaceRelative)) {
    throw new Error('browser file upload must stay inside the workspace')
  }
  if (!(await stat(file)).isFile()) throw new Error('browser file upload path must be a file')
  return file
}

function required(value, name) {
  if (value === undefined || value === null || value === '') throw new Error(`${name} is required`)
  return value
}

function optional(value) {
  return value === undefined || value === null || value === '' ? [] : [value]
}

function appendCliOption(argv, name, value) {
  if (value === undefined || value === null) return
  if (Array.isArray(value)) {
    for (const item of value) argv.push(`--${name}=${String(item)}`)
    return
  }
  if (typeof value === 'boolean') {
    argv.push(value ? `--${name}` : `--no-${name}`)
    return
  }
  if (typeof value === 'object') {
    argv.push(`--${name}=${JSON.stringify(value)}`)
    return
  }
  argv.push(`--${name}=${String(value)}`)
}

function parseCliOutput(output) {
  const trimmed = output.trim()
  if (!trimmed) return null
  try {
    return JSON.parse(trimmed)
  } catch {
    return trimmed
  }
}

function runNode(argv, env, { allowFailure = false, cwd } = {}) {
  return new Promise((resolve, reject) => {
    const child = spawn(process.execPath, argv, {
      cwd,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
      windowsHide: true,
    })
    const stdout = []
    const stderr = []
    let stdoutBytes = 0
    let stderrBytes = 0
    const limit = 8 * 1024 * 1024
    const timer = setTimeout(() => {
      void terminateProcess(child)
      reject(new Error('browser CLI request timed out'))
    }, 75_000)
    child.stdout.on('data', (chunk) => {
      stdoutBytes += chunk.length
      if (stdoutBytes <= limit) stdout.push(chunk)
    })
    child.stderr.on('data', (chunk) => {
      stderrBytes += chunk.length
      if (stderrBytes <= limit) stderr.push(chunk)
    })
    child.once('error', (error) => {
      clearTimeout(timer)
      reject(error)
    })
    child.once('exit', (code) => {
      clearTimeout(timer)
      const out = Buffer.concat(stdout).toString('utf8')
      const err = Buffer.concat(stderr).toString('utf8').trim()
      if (stdoutBytes > limit || stderrBytes > limit) {
        reject(new Error('browser CLI response exceeded 8 MiB'))
      } else if (code === 0 || allowFailure) {
        resolve(out)
      } else {
        reject(new Error(err || `browser CLI stopped with exit code ${code}`))
      }
    })
  })
}

async function terminateProcess(child) {
  if (!child || child.exitCode !== null) return
  child.kill('SIGTERM')
  await Promise.race([
    new Promise((resolve) => child.once('exit', resolve)),
    delay(3_000).then(() => {
      if (child.exitCode === null) child.kill('SIGKILL')
    }),
  ])
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds))
}

function writeResponse(response) {
  process.stdout.write(`${JSON.stringify(response)}\n`)
}

function writeFatalError(error) {
  process.stderr.write(`${error instanceof Error ? error.stack : String(error)}\n`)
}
