import { isGeoJsonArtifact, isTextMediaType } from '../model'
import {
  type ArtifactRendererPlugin,
  ArtifactRendererRegistry,
  installArtifactRendererPlugins,
} from '../registry'
import {
  AudioArtifactView,
  FallbackArtifactView,
  GeoJsonArtifactView,
  ImageArtifactView,
  TextArtifactView,
  VideoArtifactView,
} from './views'

export const artifactRendererRegistry = new ArtifactRendererRegistry()

const allSurfaces = ['inline', 'card', 'workbench'] as const

function views(component: typeof TextArtifactView) {
  return Object.fromEntries(allSurfaces.map((surface) => [surface, component]))
}

artifactRendererRegistry.register({
  id: 'builtin.geojson',
  matcher: { test: isGeoJsonArtifact },
  priority: 100,
  source: 'builtin',
  views: views(GeoJsonArtifactView),
})

artifactRendererRegistry.register({
  id: 'builtin.image',
  matcher: { mediaTypes: ['image/*'] },
  source: 'builtin',
  views: views(ImageArtifactView),
})

artifactRendererRegistry.register({
  id: 'builtin.video',
  matcher: { artifactKinds: ['video'], mediaTypes: ['video/*'] },
  source: 'builtin',
  views: views(VideoArtifactView),
})

artifactRendererRegistry.register({
  id: 'builtin.audio',
  matcher: { artifactKinds: ['audio'], mediaTypes: ['audio/*'] },
  source: 'builtin',
  views: views(AudioArtifactView),
})

artifactRendererRegistry.register({
  id: 'builtin.text',
  matcher: { test: (artifact) => isTextMediaType(artifact.mediaType) },
  source: 'builtin',
  views: views(TextArtifactView),
})

artifactRendererRegistry.register({
  id: 'builtin.fallback',
  matcher: { mediaTypes: ['*/*'] },
  priority: -1_000,
  source: 'builtin',
  views: views(FallbackArtifactView),
})

// Frontend renderer plugins are bundled at build time. Installed backend capability plugins are
// deliberately not executed as UI code.
const bundledArtifactRendererPlugins: ArtifactRendererPlugin[] = []

installArtifactRendererPlugins(artifactRendererRegistry, bundledArtifactRendererPlugins)
