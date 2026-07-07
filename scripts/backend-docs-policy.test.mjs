import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import test from 'node:test'
import {
  registeredTauriCommands,
  tauriCommandNames,
  workspaceDependencyLayerViolations,
} from './backend-docs-policy.mjs'

test('tauri command parser accepts intervening function attributes', () => {
  const commands = tauriCommandNames([
    `
#[tauri::command]
#[tracing::instrument(skip_all)]
pub async fn harness_healthcheck() -> Result<(), ()> {
  Ok(())
}

#[tauri::command(rename_all = "snake_case")]
#[cfg_attr(test, allow(dead_code))]
pub fn get_app_info() {}
`,
  ])

  assert.deepEqual(commands, ['get_app_info', 'harness_healthcheck'])
})

test('tauri handler parser reads every generate_handler invocation', () => {
  const commands = registeredTauriCommands(`
tauri::Builder::default()
  .invoke_handler(tauri::generate_handler![commands::get_app_info])
  .setup(|app| {
    let _ = tauri::generate_handler![harness_healthcheck];
    Ok(())
  });
`)

  assert.deepEqual(commands, ['get_app_info', 'harness_healthcheck'])
})

test('workspace dependency direction rejects lower layer depending on higher layer', () => {
  const metadata = {
    packages: [
      {
        id: 'path+file:///repo/crates/jyowo-harness-memory#0.1.0',
        name: 'jyowo-harness-memory',
        source: null,
      },
      {
        id: 'path+file:///repo/crates/jyowo-harness-engine#0.1.0',
        name: 'jyowo-harness-engine',
        source: null,
      },
    ],
    resolve: {
      nodes: [
        {
          id: 'path+file:///repo/crates/jyowo-harness-memory#0.1.0',
          deps: [{ pkg: 'path+file:///repo/crates/jyowo-harness-engine#0.1.0' }],
        },
        {
          id: 'path+file:///repo/crates/jyowo-harness-engine#0.1.0',
          deps: [],
        },
      ],
    },
  }

  const violations = workspaceDependencyLayerViolations(metadata, {
    'jyowo-harness-memory': 'L1',
    'jyowo-harness-engine': 'L3',
  })

  assert.deepEqual(violations, [
    {
      packageName: 'jyowo-harness-memory',
      packageLayer: 'L1',
      dependencyName: 'jyowo-harness-engine',
      dependencyLayer: 'L3',
    },
  ])
})

test('backend docs gate requires harness sandbox architecture doc', () => {
  const checker = readFileSync(new URL('./check-backend-docs.mjs', import.meta.url), 'utf8')

  assert.match(checker, /docs\/architecture\/harness\/crates\/harness-sandbox\.md/)
})
