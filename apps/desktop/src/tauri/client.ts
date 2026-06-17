import { invoke } from '@tauri-apps/api/core'

export interface AppInfo {
  name: 'Jyowo'
  version: string
  shell: 'tauri2-react'
  harness: {
    sdkCrate: 'jyowo_harness_sdk'
    mode: 'in-process'
  }
}

export interface HarnessHealthcheck {
  status: 'available'
  sdkCrate: 'jyowo_harness_sdk'
}

export function getAppInfo(): Promise<AppInfo> {
  return invoke<AppInfo>('get_app_info')
}

export function getHarnessHealthcheck(): Promise<HarnessHealthcheck> {
  return invoke<HarnessHealthcheck>('harness_healthcheck')
}
