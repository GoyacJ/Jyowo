import { FileText, Map as MapIcon, Minus, Plus, RotateCcw } from 'lucide-react'
import { useId, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import type { ArtifactViewProps } from '../resource'

export function TextArtifactView({ artifact, resource, surface }: ArtifactViewProps) {
  const { t } = useTranslation('tasks')
  const text = resource.text ?? artifact.preview
  if (!text) return <EmptyPreview />
  return (
    <section aria-label={t('workbench.artifact.textPreview', { title: artifact.title })}>
      <pre
        className={`overflow-auto whitespace-pre-wrap break-words font-mono text-xs leading-5 ${
          surface === 'workbench' ? 'min-h-full p-4' : 'max-h-72 rounded-md bg-muted/35 p-3'
        }`}
      >
        {text}
      </pre>
    </section>
  )
}

export function ImageArtifactView({ artifact, resource, surface }: ArtifactViewProps) {
  if (!resource.objectUrl) return <EmptyPreview />
  return (
    <figure
      className={`flex flex-col items-center justify-center gap-3 bg-muted/20 ${
        surface === 'workbench' ? 'min-h-full p-4' : 'overflow-hidden rounded-lg p-2'
      }`}
    >
      <img
        alt={artifact.title}
        className={
          surface === 'workbench'
            ? 'max-h-full max-w-full object-contain'
            : 'max-h-96 max-w-full rounded object-contain'
        }
        src={resource.objectUrl}
      />
      {surface === 'workbench' ? (
        <figcaption className="font-mono text-[11px] text-muted-foreground">
          {resource.mediaType}
        </figcaption>
      ) : null}
    </figure>
  )
}

export function VideoArtifactView({ artifact, resource, surface }: ArtifactViewProps) {
  const { t } = useTranslation('tasks')
  const descriptionId = useId()
  if (!resource.objectUrl) return <EmptyPreview />
  return (
    <figure className={surface === 'workbench' ? 'flex min-h-full items-center bg-black p-3' : ''}>
      {/* biome-ignore lint/a11y/useMediaCaption: Caption tracks require an explicit caption resource; an empty synthetic track is misleading. */}
      <video
        aria-describedby={descriptionId}
        aria-label={t('workbench.artifact.videoPreview', { title: artifact.title })}
        className={`w-full rounded-md bg-black ${surface === 'workbench' ? 'max-h-full' : 'max-h-96'}`}
        controls
        preload="metadata"
        src={resource.objectUrl}
      />
      <figcaption className="sr-only" id={descriptionId}>
        {t('workbench.artifact.captionsUnavailable')}
      </figcaption>
    </figure>
  )
}

export function AudioArtifactView({ artifact, resource, surface }: ArtifactViewProps) {
  const { t } = useTranslation('tasks')
  const descriptionId = useId()
  if (!resource.objectUrl) return <EmptyPreview />
  return (
    <figure
      className={`flex flex-col justify-center gap-3 ${
        surface === 'workbench' ? 'min-h-56 p-6' : 'rounded-lg bg-muted/25 p-3'
      }`}
    >
      <figcaption className="truncate text-sm">{artifact.title}</figcaption>
      {/* biome-ignore lint/a11y/useMediaCaption: Transcript tracks require an explicit resource; an empty synthetic track is misleading. */}
      <audio
        aria-describedby={descriptionId}
        aria-label={t('workbench.artifact.audioPreview', { title: artifact.title })}
        className="w-full"
        controls
        preload="metadata"
        src={resource.objectUrl}
      />
      <p className="sr-only" id={descriptionId}>
        {t('workbench.artifact.captionsUnavailable')}
      </p>
    </figure>
  )
}

export function GeoJsonArtifactView({ artifact, resource, surface }: ArtifactViewProps) {
  const { t } = useTranslation('tasks')
  const [zoom, setZoom] = useState(1)
  const map = useMemo(
    () => parseGeoJson(resource.text ?? artifact.preview ?? ''),
    [artifact.preview, resource.text],
  )
  if (map.status !== 'ready') {
    return (
      <div
        className="flex min-h-32 items-center justify-center gap-2 text-muted-foreground text-sm"
        role="status"
      >
        <MapIcon aria-hidden="true" className="size-5" />
        {t(
          map.status === 'too_complex'
            ? 'workbench.artifact.mapTooComplex'
            : 'workbench.artifact.mapInvalid',
        )}
      </div>
    )
  }
  const heightClass = surface === 'workbench' ? 'min-h-[360px] h-full' : 'h-72'
  return (
    <figure
      className={`relative overflow-hidden rounded-lg border border-border bg-muted/20 ${heightClass}`}
    >
      <svg
        aria-label={t('workbench.artifact.mapPreview', { title: artifact.title })}
        className="h-full w-full"
        role="img"
        viewBox={`0 0 ${map.width} ${map.height}`}
      >
        <title>{artifact.title}</title>
        <desc>{t('workbench.artifact.mapDescription')}</desc>
        <g
          transform={`translate(${map.width / 2} ${map.height / 2}) scale(${zoom}) translate(${-map.width / 2} ${-map.height / 2})`}
        >
          {map.elements}
        </g>
      </svg>
      <div className="absolute top-2 right-2 flex overflow-hidden rounded-md border border-border bg-background/95 shadow-sm">
        <MapControl
          label={t('workbench.artifact.mapZoomIn')}
          onClick={() => setZoom((value) => Math.min(8, value * 1.5))}
        >
          <Plus aria-hidden="true" className="size-3.5" />
        </MapControl>
        <MapControl
          label={t('workbench.artifact.mapZoomOut')}
          onClick={() => setZoom((value) => Math.max(1, value / 1.5))}
        >
          <Minus aria-hidden="true" className="size-3.5" />
        </MapControl>
        <MapControl label={t('workbench.artifact.mapReset')} onClick={() => setZoom(1)}>
          <RotateCcw aria-hidden="true" className="size-3.5" />
        </MapControl>
      </div>
    </figure>
  )
}

export function FallbackArtifactView({ artifact, resource }: ArtifactViewProps) {
  const { t } = useTranslation('tasks')
  return (
    <div className="flex min-h-32 flex-col items-center justify-center gap-2 px-6 text-center">
      <FileText aria-hidden="true" className="size-6 text-muted-foreground" />
      <p className="text-sm">{t('workbench.artifact.unsupported')}</p>
      <p className="max-w-full truncate font-mono text-[11px] text-muted-foreground">
        {artifact.title}
      </p>
      <p className="font-mono text-[11px] text-muted-foreground">
        {resource.mediaType}
        {resource.size === null
          ? ''
          : ` · ${t('workbench.artifact.bytes', { count: resource.size })}`}
      </p>
    </div>
  )
}

function EmptyPreview() {
  const { t } = useTranslation('tasks')
  return (
    <div className="flex min-h-28 items-center justify-center text-muted-foreground text-sm">
      {t('workbench.empty.artifact')}
    </div>
  )
}

function MapControl({
  children,
  label,
  onClick,
}: {
  children: React.ReactNode
  label: string
  onClick: () => void
}) {
  return (
    <button
      aria-label={label}
      className="grid size-8 place-items-center border-border border-l first:border-l-0 hover:bg-muted"
      onClick={onClick}
      type="button"
    >
      {children}
    </button>
  )
}

type Position = [number, number]
type Geometry = {
  coordinates?: unknown
  geometries?: unknown
  type: string
}

const MAX_GEOJSON_CHARACTERS = 2_000_000
const MAX_GEOJSON_DEPTH = 32
const MAX_GEOJSON_GEOMETRIES = 5_000
const MAX_GEOJSON_POSITIONS = 50_000

type GeoJsonPreview =
  | { status: 'invalid' | 'too_complex' }
  | { elements: React.ReactNode[]; height: number; status: 'ready'; width: number }

function parseGeoJson(value: string): GeoJsonPreview {
  if (!value.trim()) return { status: 'invalid' }
  if (value.length > MAX_GEOJSON_CHARACTERS) return { status: 'too_complex' }
  let parsed: unknown
  try {
    parsed = JSON.parse(value)
  } catch {
    return { status: 'invalid' }
  }
  const geometries = extractGeometries(parsed)
  if (!geometries) return { status: 'too_complex' }
  const positions: Position[] = []
  for (const geometry of geometries) {
    if (!collectPositions(geometry.coordinates, positions, 0)) {
      return { status: 'too_complex' }
    }
  }
  if (positions.length === 0) return { status: 'invalid' }
  const width = 800
  const height = 480
  const padding = 28
  const longitude = positions.map(([x]) => x)
  const latitude = positions.map(([, y]) => y)
  let minX = Math.min(...longitude)
  let maxX = Math.max(...longitude)
  let minY = Math.min(...latitude)
  let maxY = Math.max(...latitude)
  if (minX === maxX) {
    minX -= 0.5
    maxX += 0.5
  }
  if (minY === maxY) {
    minY -= 0.5
    maxY += 0.5
  }
  const rangeX = maxX - minX
  const rangeY = maxY - minY
  const scale = Math.min((width - padding * 2) / rangeX, (height - padding * 2) / rangeY)
  const drawWidth = rangeX * scale
  const drawHeight = rangeY * scale
  const offsetX = (width - drawWidth) / 2
  const offsetY = (height - drawHeight) / 2
  const project = ([x, y]: Position): Position => [
    offsetX + (x - minX) * scale,
    height - (offsetY + (y - minY) * scale),
  ]
  return {
    elements: geometries.flatMap((geometry, index) =>
      geometryElements(geometry, project, `geometry-${index}`),
    ),
    height,
    status: 'ready',
    width,
  }
}

function extractGeometries(value: unknown): Geometry[] | null {
  const geometries: Geometry[] = []
  return collectGeometries(value, geometries, 0) ? geometries : null
}

function collectGeometries(value: unknown, geometries: Geometry[], depth: number): boolean {
  if (depth > MAX_GEOJSON_DEPTH || geometries.length >= MAX_GEOJSON_GEOMETRIES) return false
  const root = objectValue(value)
  if (!root || typeof root.type !== 'string') return true
  if (root.type === 'FeatureCollection') {
    if (!Array.isArray(root.features)) return true
    for (const feature of root.features) {
      if (!collectGeometries(feature, geometries, depth + 1)) return false
    }
    return true
  }
  if (root.type === 'Feature') {
    return root.geometry ? collectGeometries(root.geometry, geometries, depth + 1) : true
  }
  if (root.type === 'GeometryCollection') {
    if (!Array.isArray(root.geometries)) return true
    for (const geometry of root.geometries) {
      if (!collectGeometries(geometry, geometries, depth + 1)) return false
    }
    return true
  }
  geometries.push({ coordinates: root.coordinates, geometries: root.geometries, type: root.type })
  return geometries.length <= MAX_GEOJSON_GEOMETRIES
}

function collectPositions(value: unknown, output: Position[], depth: number): boolean {
  if (depth > MAX_GEOJSON_DEPTH) return false
  const coordinate = position(value)
  if (coordinate) {
    if (output.length >= MAX_GEOJSON_POSITIONS) return false
    output.push(coordinate)
    return true
  }
  if (!Array.isArray(value)) return true
  for (const child of value) {
    if (!collectPositions(child, output, depth + 1)) return false
  }
  return true
}

function geometryElements(
  geometry: Geometry,
  project: (position: Position) => Position,
  key: string,
): React.ReactNode[] {
  const stroke = 'var(--color-primary)'
  const fill = 'color-mix(in srgb, var(--color-primary) 24%, transparent)'
  if (geometry.type === 'Point') {
    const point = position(geometry.coordinates)
    if (!point) return []
    const [x, y] = project(point)
    return [<circle cx={x} cy={y} fill={stroke} key={key} r="6" stroke="white" strokeWidth="2" />]
  }
  if (geometry.type === 'MultiPoint') {
    return positions(geometry.coordinates).map((point, index) => {
      const [x, y] = project(point)
      return (
        <circle
          cx={x}
          cy={y}
          fill={stroke}
          key={`${key}-${index}`}
          r="5"
          stroke="white"
          strokeWidth="1.5"
        />
      )
    })
  }
  if (geometry.type === 'LineString') {
    return [
      <polyline
        fill="none"
        key={key}
        points={pointList(geometry.coordinates, project)}
        stroke={stroke}
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="3"
      />,
    ]
  }
  if (geometry.type === 'MultiLineString') {
    return arrays(geometry.coordinates).map((line, index) => (
      <polyline
        fill="none"
        key={`${key}-${index}`}
        points={pointList(line, project)}
        stroke={stroke}
        strokeLinecap="round"
        strokeLinejoin="round"
        strokeWidth="3"
      />
    ))
  }
  if (geometry.type === 'Polygon') {
    return [
      <path
        d={polygonPath(geometry.coordinates, project)}
        fill={fill}
        fillRule="evenodd"
        key={key}
        stroke={stroke}
        strokeLinejoin="round"
        strokeWidth="2"
      />,
    ]
  }
  if (geometry.type === 'MultiPolygon') {
    return arrays(geometry.coordinates).map((polygon, index) => (
      <path
        d={polygonPath(polygon, project)}
        fill={fill}
        fillRule="evenodd"
        key={`${key}-${index}`}
        stroke={stroke}
        strokeLinejoin="round"
        strokeWidth="2"
      />
    ))
  }
  if (geometry.type === 'GeometryCollection' && Array.isArray(geometry.geometries)) {
    return geometry.geometries.flatMap((value, index) => {
      const entries = extractGeometries(value) ?? []
      return entries.flatMap((entry, entryIndex) =>
        geometryElements(entry, project, `${key}-${index}-${entryIndex}`),
      )
    })
  }
  return []
}

function polygonPath(value: unknown, project: (position: Position) => Position) {
  return arrays(value)
    .map((ring) => {
      const points = positions(ring).map(project)
      return points.length > 0
        ? `${points.map(([x, y], index) => `${index === 0 ? 'M' : 'L'}${x},${y}`).join(' ')} Z`
        : ''
    })
    .join(' ')
}

function pointList(value: unknown, project: (position: Position) => Position) {
  return positions(value)
    .map(project)
    .map(([x, y]) => `${x},${y}`)
    .join(' ')
}

function positions(value: unknown) {
  return arrays(value)
    .map(position)
    .filter((value): value is Position => value !== null)
}

function arrays(value: unknown): unknown[][] {
  return Array.isArray(value) ? value.filter(Array.isArray) : []
}

function position(value: unknown): Position | null {
  if (!Array.isArray(value) || value.length < 2) return null
  const [x, y] = value
  return typeof x === 'number' && Number.isFinite(x) && typeof y === 'number' && Number.isFinite(y)
    ? [x, y]
    : null
}

function objectValue(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null
}
