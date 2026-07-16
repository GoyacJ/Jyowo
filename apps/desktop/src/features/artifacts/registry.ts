import type { ComponentType } from 'react'

import type { ArtifactDescriptor, ArtifactSurface } from './model'
import type { ArtifactViewProps } from './resource'

type ArtifactRendererMatcher = {
  artifactKinds?: string[]
  formats?: string[]
  mediaTypes?: string[]
  test?: (artifact: ArtifactDescriptor) => boolean
}

export type ArtifactRendererDefinition = {
  id: string
  matcher: ArtifactRendererMatcher
  priority?: number
  source?: 'builtin' | `plugin:${string}`
  views: Partial<Record<ArtifactSurface, ComponentType<ArtifactViewProps>>>
}

export type PluginArtifactRendererDefinition = Omit<ArtifactRendererDefinition, 'id' | 'source'> & {
  id: string
}

export type ArtifactRendererPlugin = {
  id: string
  renderers: PluginArtifactRendererDefinition[]
}

export class ArtifactRendererRegistry {
  readonly #renderers = new Map<string, ArtifactRendererDefinition>()

  register(renderer: ArtifactRendererDefinition) {
    validateIdentifier(renderer.id, 'artifact renderer')
    if (this.#renderers.has(renderer.id)) {
      throw new Error(`Artifact renderer already registered: ${renderer.id}`)
    }
    this.#renderers.set(renderer.id, renderer)
    let active = true
    return () => {
      if (!active) return false
      active = false
      if (this.#renderers.get(renderer.id) !== renderer) return false
      return this.#renderers.delete(renderer.id)
    }
  }

  registerPlugin(pluginId: string, renderers: PluginArtifactRendererDefinition[]) {
    validateIdentifier(pluginId, 'artifact renderer plugin')
    const definitions = renderers.map((renderer) => ({
      ...renderer,
      id: `plugin:${pluginId}/${renderer.id}`,
      source: `plugin:${pluginId}` as const,
    }))
    const identifiers = new Set<string>()
    for (const [index, definition] of definitions.entries()) {
      validateIdentifier(renderers[index]?.id ?? '', 'artifact renderer')
      if (identifiers.has(definition.id) || this.#renderers.has(definition.id)) {
        throw new Error(`Artifact renderer already registered: ${definition.id}`)
      }
      identifiers.add(definition.id)
    }
    for (const definition of definitions) this.#renderers.set(definition.id, definition)
    let active = true
    return () => {
      if (!active) return
      active = false
      for (const definition of definitions) {
        if (this.#renderers.get(definition.id) === definition) {
          this.#renderers.delete(definition.id)
        }
      }
    }
  }

  resolve(artifact: ArtifactDescriptor, surface: ArtifactSurface) {
    return [...this.#renderers.values()]
      .flatMap((renderer) => {
        if (!renderer.views[surface]) return []
        const match = evaluateMatcher(renderer.matcher, artifact)
        return match?.matched
          ? [{ renderer, score: match.specificity + (renderer.priority ?? 0) }]
          : []
      })
      .sort(
        (left, right) =>
          right.score - left.score || left.renderer.id.localeCompare(right.renderer.id),
      )
      .at(0)?.renderer
  }

  list() {
    return [...this.#renderers.values()]
  }
}

export function installArtifactRendererPlugins(
  registry: ArtifactRendererRegistry,
  plugins: ArtifactRendererPlugin[],
) {
  const cleanup: Array<() => void> = []
  try {
    for (const plugin of plugins) {
      cleanup.push(registry.registerPlugin(plugin.id, plugin.renderers))
    }
  } catch (error) {
    for (const unregister of cleanup.reverse()) unregister()
    throw error
  }
  let active = true
  return () => {
    if (!active) return
    active = false
    for (const unregister of cleanup.reverse()) unregister()
  }
}

function validateIdentifier(identifier: string, kind: string) {
  if (!/^[a-z0-9][a-z0-9._-]*$/i.test(identifier)) {
    throw new Error(`Invalid ${kind} identifier: ${identifier}`)
  }
}

function evaluateMatcher(matcher: ArtifactRendererMatcher, artifact: ArtifactDescriptor) {
  const results: boolean[] = []
  let specificity = 0
  if (matcher.mediaTypes) {
    let matched = false
    for (const mediaType of matcher.mediaTypes) {
      if (mediaType === artifact.mediaType) {
        matched = true
        specificity = Math.max(specificity, 400)
      } else if (mediaTypeMatches(mediaType, artifact.mediaType)) {
        matched = true
        specificity = Math.max(specificity, 300)
      }
    }
    results.push(matched)
  }
  if (matcher.formats) {
    const matched = Boolean(artifact.format && matcher.formats.includes(artifact.format))
    results.push(matched)
    if (matched) specificity = Math.max(specificity, 250)
  }
  if (matcher.artifactKinds) {
    const matched = Boolean(
      artifact.artifactKind && matcher.artifactKinds.includes(artifact.artifactKind),
    )
    results.push(matched)
    if (matched) specificity = Math.max(specificity, 200)
  }
  if (matcher.test) {
    try {
      const matched = matcher.test(artifact)
      results.push(matched)
      if (matched) specificity = Math.max(specificity, 100)
    } catch {
      return null
    }
  }
  return { matched: results.length === 0 || results.some(Boolean), specificity }
}

function mediaTypeMatches(pattern: string, mediaType: string) {
  if (pattern === '*/*') return true
  if (pattern.endsWith('/*')) return mediaType.startsWith(pattern.slice(0, -1))
  return pattern === mediaType
}
