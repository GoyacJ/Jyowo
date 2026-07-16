import { createContext, useCallback, useContext, useEffect, useState } from 'react'

import type { DaemonClient } from '@/shared/daemon/client'

import {
  type ArtifactDescriptor,
  type ArtifactSurface,
  isGeoJsonArtifact,
  isTextMediaType,
  normalizeArtifactDescriptor,
  normalizeMediaType,
} from './model'

export type ArtifactBlobLoader = Pick<DaemonClient, 'readBlob'>['readBlob']

export type ArtifactResource = {
  bytes: Uint8Array | null
  error: boolean
  loading: boolean
  mediaType: string
  missing: boolean
  objectUrl: string | null
  retry: () => void
  size: number | null
  text: string | null
}

export type ArtifactViewProps = {
  artifact: ArtifactDescriptor
  resource: ArtifactResource
  surface: ArtifactSurface
}

type ArtifactResourceState = {
  loader: ArtifactBlobLoader | undefined
  requestKey: string
  resource: Omit<ArtifactResource, 'retry'>
}

const ArtifactBlobLoaderContext = createContext<ArtifactBlobLoader | undefined>(undefined)

export function ArtifactResourceProvider({
  children,
  loader,
}: {
  children: React.ReactNode
  loader?: ArtifactBlobLoader
}) {
  return (
    <ArtifactBlobLoaderContext.Provider value={loader}>
      {children}
    </ArtifactBlobLoaderContext.Provider>
  )
}

export function useArtifactResource(
  descriptor: ArtifactDescriptor,
  explicitLoader?: ArtifactBlobLoader,
  surface: ArtifactSurface = 'workbench',
): ArtifactResource {
  const contextLoader = useContext(ArtifactBlobLoaderContext)
  const loader = explicitLoader ?? contextLoader
  const artifact = normalizeArtifactDescriptor(descriptor)
  const [retryVersion, setRetryVersion] = useState(0)
  const blobIds = resourceBlobIds(artifact, surface)
  const requestKey = resourceRequestKey(artifact, surface, retryVersion)
  const [state, setState] = useState<ArtifactResourceState>(() => ({
    loader,
    requestKey,
    resource: emptyResource(artifact),
  }))

  useEffect(() => {
    if (blobIds.length === 0 || !loader) {
      setState({ loader, requestKey, resource: emptyResource(artifact) })
      return
    }
    let cancelled = false
    let objectUrl: string | null = null
    setState({
      loader,
      requestKey,
      resource: { ...emptyResource(artifact), loading: true },
    })
    void (async () => {
      let lastFailure: 'error' | 'missing' = 'missing'
      for (const blobId of blobIds) {
        try {
          const blob = await loader(blobId)
          if (cancelled) return
          const bytes = blob.bytes
          if (blob.missing || bytes === null || bytes.byteLength === 0) {
            lastFailure = 'missing'
            continue
          }
          const mediaType = normalizeMediaType(blob.mediaType || artifact.mediaType)
          if (needsObjectUrl(mediaType)) {
            objectUrl = URL.createObjectURL(new Blob([Uint8Array.from(bytes)], { type: mediaType }))
          }
          setState({
            loader,
            requestKey,
            resource: {
              bytes,
              error: false,
              loading: false,
              mediaType,
              missing: false,
              objectUrl,
              size: blob.size,
              text:
                isTextMediaType(mediaType) || isGeoJsonArtifact({ ...artifact, mediaType })
                  ? new TextDecoder().decode(bytes)
                  : (artifact.preview ?? null),
            },
          })
          return
        } catch {
          lastFailure = 'error'
        }
      }
      if (!cancelled) {
        setState({
          loader,
          requestKey,
          resource: {
            ...emptyResource(artifact),
            error: lastFailure === 'error',
            missing: lastFailure === 'missing',
          },
        })
      }
    })()
    return () => {
      cancelled = true
      if (objectUrl) URL.revokeObjectURL(objectUrl)
    }
  }, [loader, requestKey])

  const retry = useCallback(() => setRetryVersion((version) => version + 1), [])
  const resource =
    state.loader === loader && state.requestKey === requestKey
      ? state.resource
      : {
          ...emptyResource(artifact),
          loading: Boolean(loader && blobIds.length > 0),
        }
  return { ...resource, retry }
}

function resourceBlobIds(artifact: ArtifactDescriptor, surface: ArtifactSurface) {
  const preferred =
    surface === 'workbench'
      ? [artifact.blobId, artifact.presentation?.previewBlobId]
      : [artifact.presentation?.previewBlobId, artifact.blobId]
  return preferred.filter(
    (blobId, index): blobId is string => Boolean(blobId) && preferred.indexOf(blobId) === index,
  )
}

function resourceRequestKey(
  artifact: ArtifactDescriptor,
  surface: ArtifactSurface,
  retryVersion: number,
) {
  return JSON.stringify([
    artifact.artifactId,
    artifact.artifactKind,
    artifact.blobId,
    artifact.format,
    artifact.mediaType,
    artifact.presentation?.preferredSurface,
    artifact.presentation?.previewBlobId,
    artifact.preview,
    artifact.size,
    artifact.title,
    surface,
    retryVersion,
  ])
}

function emptyResource(artifact: ArtifactDescriptor): Omit<ArtifactResource, 'retry'> {
  return {
    bytes: null,
    error: false,
    loading: false,
    mediaType: normalizeMediaType(artifact.mediaType),
    missing: false,
    objectUrl: null,
    size: artifact.size ?? null,
    text: artifact.preview ?? null,
  }
}

function needsObjectUrl(mediaType: string) {
  return (
    mediaType.startsWith('image/') ||
    mediaType.startsWith('video/') ||
    mediaType.startsWith('audio/')
  )
}
