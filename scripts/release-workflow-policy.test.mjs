import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { dirname } from 'node:path'
import { fileURLToPath } from 'node:url'
import test from 'node:test'

const repoRoot = dirname(dirname(fileURLToPath(import.meta.url)))
const workflow = readFileSync(join(repoRoot, '.github', 'workflows', 'release.yml'), 'utf8')
const ciWorkflow = readFileSync(join(repoRoot, '.github', 'workflows', 'ci.yml'), 'utf8')
const browserRuntimePrepare = readFileSync(
  join(repoRoot, 'apps', 'browser-runtime', 'scripts', 'prepare-runtime.mjs'),
  'utf8',
)

function ciJob(jobName) {
  return ciWorkflow.match(new RegExp(`\\n  ${jobName}:[\\s\\S]*?(?=\\n  [a-zA-Z0-9_-]+:\\n|\\n*$)`))?.[0] ?? ''
}

test('release workflow is triggered only by semantic version tags', () => {
  assert.match(workflow, /push:\s*\n\s*tags:\s*\n\s*-\s*['"]v\*\.\*\.\*['"]/)
})

test('release workflow checks versions before matrix builds', () => {
  assert.match(workflow, /needs:\s*version/)
  assert.match(workflow, /pnpm check:release-version/)
  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY is not configured/)
  assert.match(workflow, /pnpm check:release-workflow/)
  assert.match(workflow, /pnpm check:tauri-updater/)
})

test('release workflow pins the browser runtime Node.js version', () => {
  const setupNodeVersions = [...workflow.matchAll(/node-version:\s*([^\s]+)/g)].map(
    ([, version]) => version,
  )

  assert.deepEqual(setupNodeVersions, ['24.12.0', '24.12.0', '24.12.0'])
})

test('browser runtime packaging resolves the pnpm Windows command shim', () => {
  assert.match(
    browserRuntimePrepare,
    /process\.platform === 'win32' \? 'pnpm\.cmd' : 'pnpm'/,
  )
  assert.match(browserRuntimePrepare, /run\(\s*pnpmCommand,/)
})

test('release workflow verifies the published updater manifest after all builds', () => {
  assert.match(workflow, /verify:\s*\n\s*name:\s*verify release/)
  assert.match(workflow, /needs:\s*build/)
  assert.match(workflow, /node scripts\/verify-release-assets\.mjs "\$GITHUB_REF_NAME"/)
})

test('release workflow builds all supported desktop platforms', () => {
  assert.match(workflow, /windows-latest/)
  assert.match(workflow, /macos-latest/)
  assert.match(workflow, /ubuntu-22\.04/)
  assert.match(workflow, /libwebkit2gtk-4\.1-dev/)
  assert.match(workflow, /libayatana-appindicator3-dev/)
})

test('release workflow uploads Tauri artifacts with updater signing secrets', () => {
  assert.match(workflow, /tauri-apps\/tauri-action@v0/)
  assert.match(workflow, /projectPath:\s*apps\/desktop/)
  assert.match(workflow, /tauriScript:\s*pnpm tauri:release/)
  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY:/)
  assert.match(workflow, /TAURI_SIGNING_PRIVATE_KEY_PASSWORD:/)
  assert.match(workflow, /includeUpdaterJson:\s*true/)
  assert.doesNotMatch(workflow, /uploadUpdaterJson:/)
  assert.match(workflow, /releaseDraft:\s*false/)
})

test('ci workflow runs fast gates on pull requests', () => {
  assert.match(ciWorkflow, /workflow_dispatch:/)
  assert.match(ciWorkflow, /pull_request:/)
  assert.match(ciJob('policy-fast'), /if:\s*github\.event_name == 'pull_request'/)
  assert.match(
    ciJob('policy-fast'),
    /pnpm check:release-version && pnpm check:release-workflow && pnpm check:tauri-updater && pnpm check:agent-orchestration-no-fakes && pnpm check:daemon-sidecar/,
  )
  assert.match(ciJob('test-architecture'), /if:\s*github\.event_name == 'pull_request'[\s\S]*pnpm check:test-architecture/)
  assert.match(ciJob('frontend-fast'), /if:\s*github\.event_name == 'pull_request'[\s\S]*pnpm check:frontend:fast/)
  assert.match(ciJob('rust-fast'), /if:\s*github\.event_name == 'pull_request'[\s\S]*pnpm check:rust:fast/)
})

test('ci workflow runs full gates only on main pushes and manual dispatch', () => {
  const fullGateCondition =
    "if: github.event_name == 'workflow_dispatch' || (github.event_name == 'push' && github.ref == 'refs/heads/main')"

  for (const [jobName, command] of [
    ['frontend', 'pnpm check:desktop'],
    ['rust', 'pnpm check:rust'],
    ['desktop-build', 'pnpm check:desktop:full'],
  ]) {
    const job = ciJob(jobName)
    assert.ok(job.includes(fullGateCondition))
    assert.ok(job.includes(command))
  }
})

test('ci pnpm jobs install Node, pnpm, and dependencies', () => {
  const pnpmJobNames = [
    'policy-fast',
    'test-architecture',
    'frontend-fast',
    'rust-fast',
    'frontend',
    'rust',
    'desktop-build',
  ]

  for (const jobName of pnpmJobNames) {
    const job = ciJob(jobName)
    assert.match(job, /pnpm\/action-setup@v4/)
    assert.match(job, /version:\s*11\.7\.0/)
    assert.match(job, /actions\/setup-node@v4/)
    assert.match(job, /node-version:\s*24\.12\.0/)
    assert.match(job, /cache:\s*pnpm/)
    assert.match(job, /cache-dependency-path:\s*pnpm-lock\.yaml/)
    assert.match(job, /pnpm install --frozen-lockfile/)
  }
})

test('ci rust jobs install Rust and use cache', () => {
  for (const jobName of ['rust-fast', 'rust', 'windows-release-check', 'desktop-build']) {
    const job = ciJob(jobName)
    assert.match(job, /dtolnay\/rust-toolchain@stable/)
    assert.match(job, /swatinem\/rust-cache@v2/)
  }
})

test('ci Linux Rust jobs install Tauri system dependencies', () => {
  for (const jobName of ['rust-fast', 'rust']) {
    const job = ciJob(jobName)
    assert.match(job, /sudo apt-get update/)
    assert.match(job, /libwebkit2gtk-4\.1-dev/)
    assert.match(job, /libgtk-3-dev/)
    assert.match(job, /libayatana-appindicator3-dev/)
    assert.match(job, /librsvg2-dev/)
    assert.match(job, /patchelf/)
    assert.match(job, /bubblewrap/)
    assert.match(job, /sudo sysctl -w kernel\.apparmor_restrict_unprivileged_userns=0/)
  }
})

test('ci full Rust job limits linker resource usage', () => {
  const job = ciJob('rust')

  assert.match(job, /CARGO_BUILD_JOBS:\s*['"]2['"]/)
  assert.match(job, /CARGO_INCREMENTAL:\s*['"]0['"]/)
  assert.match(job, /CARGO_PROFILE_DEV_DEBUG:\s*['"]0['"]/)
  assert.match(job, /CARGO_PROFILE_TEST_DEBUG:\s*['"]0['"]/)
})

test('ci full desktop build installs Playwright Chromium', () => {
  const job = ciJob('desktop-build')

  assert.match(
    job,
    /pnpm install --frozen-lockfile[\s\S]*pnpm -C apps\/desktop exec playwright install chromium[\s\S]*pnpm check:desktop:full/,
  )
})

test('ci compiles the release product graph on Windows', () => {
  const job = ciJob('windows-release-check')

  assert.match(job, /runs-on:\s*windows-latest/)
  assert.match(job, /node-version:\s*24\.12\.0/)
  assert.match(job, /node scripts\/build-daemon-sidecar\.mjs/)
  assert.match(job, /cargo check -p jyowo-desktop-shell/)
})
