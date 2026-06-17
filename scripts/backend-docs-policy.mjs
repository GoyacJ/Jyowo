export function unique(values) {
  return [...new Set(values)].sort()
}

export function linesFromTextBlock(text, label) {
  const match = text.match(new RegExp(`${label}:\\s*\`\`\`text\\s*([\\s\\S]*?)\`\`\``))

  if (!match) {
    return []
  }

  return match[1]
    .split(/\r?\n/)
    .map((line) => line.trim())
    .filter(Boolean)
}

export function workspaceMembers(rootCargo) {
  const match = rootCargo.match(/\[workspace\][\s\S]*?members\s*=\s*\[([\s\S]*?)\]/)

  if (!match) {
    return []
  }

  return [...match[1].matchAll(/"([^"]+)"/g)].map((entry) => entry[1]).sort()
}

export function tomlSection(text, sectionName) {
  const start = text.indexOf(`[${sectionName}]`)

  if (start === -1) {
    return ''
  }

  const rest = text.slice(start + sectionName.length + 2)
  const nextSectionOffset = rest.search(/\n\[[^\]]+\]/)

  return nextSectionOffset === -1 ? rest : rest.slice(0, nextSectionOffset)
}

export function workspaceLayerRows(engineeringDoc) {
  const start = engineeringDoc.indexOf('## Workspace Layers')

  if (start === -1) {
    return []
  }

  const rest = engineeringDoc.slice(start)
  const nextSectionOffset = rest.slice(1).search(/\n## /)
  const section = nextSectionOffset === -1 ? rest : rest.slice(0, nextSectionOffset + 1)

  return [...section.matchAll(/^\|\s*`([^`]+)`\s*\|\s*`([^`]+)`\s*\|\s*([^|]+?)\s*\|/gm)].map(
    (match) => ({
      packageName: match[1],
      path: match[2],
      layer: match[3].trim(),
    }),
  )
}

export function normalizeMarkdownTableCell(value) {
  return value.replace(/`/g, '').replace(/\s+/g, ' ').trim()
}

export function rustDependencyPolicyRows(qualityDoc) {
  const sectionStart = qualityDoc.indexOf('Allowed upstream-held transitive dependencies:')

  if (sectionStart === -1) {
    return []
  }

  const section = qualityDoc.slice(sectionStart)
  return section
    .split(/\r?\n/)
    .filter((line) => /^\|\s*`/.test(line))
    .map((line) => {
      const cells = line
        .split('|')
        .slice(1, -1)
        .map(normalizeMarkdownTableCell)

      return {
        name: cells[0],
        current: cells[1],
        available: cells[2],
        owner: cells[3],
        constraint: cells[4],
      }
    })
}

export function tauriCommandNames(commandsSources) {
  const commandPattern =
    /#\[\s*tauri::command(?:\([^\]]*\))?\s*\](?:\s*#\[[^\]]*\])*\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z0-9_]+)/g

  return unique(
    commandsSources.flatMap((source) => [...source.matchAll(commandPattern)].map((match) => match[1])),
  )
}

export function registeredTauriCommands(libSource) {
  return unique(
    [...libSource.matchAll(/generate_handler!\s*\[([\s\S]*?)\]/g)].flatMap((handlerMatch) =>
      [
        ...handlerMatch[1].matchAll(
          /(?:^|[\s,])((?:[A-Za-z_][A-Za-z0-9_]*::)*[A-Za-z_][A-Za-z0-9_]*)\s*(?:,|$)/gm,
        ),
      ].map((match) => match[1].split('::').at(-1)),
    ),
  )
}

const layerRanks = new Map([
  ['L0', 0],
  ['L1', 1],
  ['L2', 2],
  ['L3', 3],
  ['L4', 4],
  ['Tauri shell', 5],
])

function hasNonDevDependencyKind(dep) {
  if (!dep.dep_kinds) {
    return true
  }

  return dep.dep_kinds.some((depKind) => depKind.kind !== 'dev')
}

export function workspaceDependencyLayerViolations(metadata, layersByPackage) {
  const workspacePackageById = new Map(
    metadata.packages
      .filter((pkg) => pkg.source === null && layersByPackage[pkg.name] !== undefined)
      .map((pkg) => [pkg.id, pkg]),
  )

  return (metadata.resolve?.nodes ?? []).flatMap((node) => {
    const pkg = workspacePackageById.get(node.id)

    if (!pkg) {
      return []
    }

    const packageLayer = layersByPackage[pkg.name]
    const packageRank = layerRanks.get(packageLayer)

    if (packageRank === undefined) {
      return []
    }

    return (node.deps ?? []).flatMap((dep) => {
      if (!hasNonDevDependencyKind(dep)) {
        return []
      }

      const dependency = workspacePackageById.get(dep.pkg)

      if (!dependency) {
        return []
      }

      const dependencyLayer = layersByPackage[dependency.name]
      const dependencyRank = layerRanks.get(dependencyLayer)

      if (dependencyRank === undefined || dependencyRank <= packageRank) {
        return []
      }

      return [
        {
          packageName: pkg.name,
          packageLayer,
          dependencyName: dependency.name,
          dependencyLayer,
        },
      ]
    })
  })
}
