import type { PlanItem } from './PlanBlock'

export const prototypePlanItems = [
  { label: 'Initialize project & dependencies', status: 'Done' },
  { label: 'Configure Tauri command boundary', status: 'Done' },
  { label: 'Set up React + TypeScript (Vite)', status: 'Done' },
  { label: 'Add base app shell & IPC bridge', status: 'Done' },
  { label: 'Add scripts, README, and .gitignore', status: 'In progress' },
] satisfies PlanItem[]

export const prototypeDiffLines = [
  '+ use serde::Serialize;',
  '+',
  '+ #[derive(Serialize)]',
  '+ struct AppInfoPayload {',
  '+   name: String,',
  '+   version: String,',
  '+ }',
  '+',
  '+ #[tauri::command]',
  '+ fn get_app_info() -> AppInfoPayload {',
  '+   AppInfoPayload {',
  '+     name: "Jyowo".into(),',
  '+     version: env!("CARGO_PKG_VERSION").into(),',
  '+   }',
  '+ }',
]
