import { tauriDaemonClient } from '@/shared/daemon/client'
import { tauriCommandClient } from './commands'

export function createDefaultCommandClient() {
  return tauriCommandClient
}

export function createDefaultDaemonClient() {
  return tauriDaemonClient
}
