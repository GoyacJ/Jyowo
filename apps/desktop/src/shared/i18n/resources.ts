import { enUS } from './locales/en-US'
import { zhCN } from './locales/zh-CN'

export const resources = {
  'en-US': enUS,
  'zh-CN': zhCN,
} as const

type ResourceNode = Record<string, unknown>

export function getResourceKeyPaths(resource: ResourceNode) {
  return collectResourceKeyPaths(resource).sort()
}

function collectResourceKeyPaths(resource: ResourceNode, prefix = ''): string[] {
  return Object.entries(resource).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key

    if (value !== null && typeof value === 'object' && !Array.isArray(value)) {
      return collectResourceKeyPaths(value as ResourceNode, path)
    }

    return [path]
  })
}
