import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useCallback } from 'react'

import {
  clearSkillSecret,
  deleteSkill,
  getSkillCatalogEntry,
  getSkillCatalogFile,
  getSkillConfig,
  getSkillDetail,
  getSkillFile,
  importSkill,
  installSkillFromCatalog,
  listSkillCatalogEntries,
  listSkillCatalogInstallTasks,
  listSkillCatalogSources,
  listSkills,
  type SkillCatalogEntry,
  type SkillCatalogSource,
  setSkillConfigValue,
  setSkillEnabled,
  setSkillSecret,
} from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'

export const CATALOG_PAGE_SIZE = 12

export const skillQueryKeys = {
  all: ['skills'] as const,
  catalog: () => [...skillQueryKeys.all, 'catalog'] as const,
  catalogDetail: (sourceId: string, entryId: string | null, version: string | null) =>
    [...skillQueryKeys.catalog(), 'detail', sourceId, entryId, version] as const,
  catalogEntries: (sourceId: string, query: string, cursor: string | null) =>
    [...skillQueryKeys.catalog(), 'entries', sourceId, query, cursor] as const,
  catalogFile: (
    sourceId: string,
    entryId: string | null,
    version: string | null,
    path: string | null,
  ) => [...skillQueryKeys.catalog(), 'file', sourceId, entryId, version, path] as const,
  catalogInstallTasks: () => [...skillQueryKeys.catalog(), 'installTasks'] as const,
  catalogSources: () => [...skillQueryKeys.catalog(), 'sources'] as const,
  config: (id: string) => [...skillQueryKeys.all, 'config', id] as const,
  detail: (id: string | null) => [...skillQueryKeys.all, 'detail', id] as const,
  file: (id: string | null, path: string | null) =>
    [...skillQueryKeys.all, 'file', id, path] as const,
  list: () => [...skillQueryKeys.all, 'list'] as const,
}

export type SkillCatalogSourceId = SkillCatalogSource['id']

export type CatalogInstallMutationRequest = {
  entry: SkillCatalogEntry
  operationId: string
}

export function useSkills() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.list(),
    queryFn: () => listSkills(commandClient),
  })
}

export function useSkillDetail(id: string | null) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: id !== null,
    queryKey: skillQueryKeys.detail(id),
    queryFn: () => getSkillDetail(id ?? '', commandClient),
  })
}

export function useSkillFile(id: string | null, path: string | null) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: id !== null && path !== null,
    queryKey: skillQueryKeys.file(id, path),
    queryFn: () => getSkillFile(id ?? '', path ?? '', commandClient),
  })
}

export function useImportSkill() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (sourcePath: string) => importSkill(sourcePath, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: skillQueryKeys.all })
    },
  })
}

export function useSetSkillEnabled() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ enabled, id }: { enabled: boolean; id: string }) =>
      setSkillEnabled(id, enabled, commandClient),
    onSuccess: async (response) => {
      await queryClient.invalidateQueries({ queryKey: skillQueryKeys.list() })
      await queryClient.invalidateQueries({ queryKey: skillQueryKeys.detail(response.skill.id) })
    },
  })
}

export function useDeleteSkill() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (id: string) => deleteSkill(id, commandClient),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: skillQueryKeys.all })
    },
  })
}

export function useSkillConfig(id: string) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: id.length > 0,
    queryKey: skillQueryKeys.config(id),
    queryFn: () => getSkillConfig(id, commandClient),
  })
}

async function invalidateSkillConfigQueries(
  queryClient: ReturnType<typeof useQueryClient>,
  id: string,
) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: skillQueryKeys.config(id) }),
    queryClient.invalidateQueries({ queryKey: skillQueryKeys.detail(id) }),
    queryClient.invalidateQueries({ queryKey: skillQueryKeys.list() }),
  ])
}

export function useSetSkillConfigValue() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ key, skillId, value }: { key: string; skillId: string; value: unknown }) =>
      setSkillConfigValue(skillId, key, value, commandClient),
    onSuccess: async (_response, variables) => {
      await invalidateSkillConfigQueries(queryClient, variables.skillId)
    },
  })
}

export function useSetSkillSecret() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useCallback(
    async ({ key, skillId, value }: { key: string; skillId: string; value: string }) => {
      const response = await setSkillSecret(skillId, key, value, commandClient)
      await invalidateSkillConfigQueries(queryClient, skillId)
      return response
    },
    [commandClient, queryClient],
  )
}

export function useClearSkillSecret() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: ({ key, skillId }: { key: string; skillId: string }) =>
      clearSkillSecret(skillId, key, commandClient),
    onSuccess: async (_response, variables) => {
      await invalidateSkillConfigQueries(queryClient, variables.skillId)
    },
  })
}

export function useSkillCatalogSources() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.catalogSources(),
    queryFn: () => listSkillCatalogSources(commandClient),
  })
}

export function useSkillCatalogEntries(
  sourceId: SkillCatalogSourceId,
  query: string,
  cursor: string | null,
) {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.catalogEntries(sourceId, query, cursor),
    queryFn: () =>
      listSkillCatalogEntries(
        {
          cursor: cursor ?? undefined,
          limit: CATALOG_PAGE_SIZE,
          query: query.trim() || undefined,
          sourceId,
        },
        commandClient,
      ),
  })
}

export function useSkillCatalogEntry(
  sourceId: SkillCatalogSourceId,
  entry: SkillCatalogEntry | null,
  enabled: boolean,
) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: enabled && entry !== null,
    queryKey: skillQueryKeys.catalogDetail(
      sourceId,
      entry?.entryId ?? null,
      entry?.version ?? null,
    ),
    queryFn: () =>
      getSkillCatalogEntry(
        {
          entryId: entry?.entryId ?? '',
          sourceId,
          version: entry?.version,
        },
        commandClient,
      ),
  })
}

export function useSkillCatalogFile(
  sourceId: SkillCatalogSourceId,
  entry: SkillCatalogEntry | null,
  path: string | null,
  enabled: boolean,
) {
  const commandClient = useCommandClient()

  return useQuery({
    enabled: enabled && entry !== null && path !== null,
    queryKey: skillQueryKeys.catalogFile(
      sourceId,
      entry?.entryId ?? null,
      entry?.version ?? null,
      path,
    ),
    queryFn: () =>
      getSkillCatalogFile(
        {
          entryId: entry?.entryId ?? '',
          path: path ?? '',
          sourceId,
          version: entry?.version,
        },
        commandClient,
      ),
  })
}

export function useSkillCatalogInstallTasks() {
  const commandClient = useCommandClient()

  return useQuery({
    queryKey: skillQueryKeys.catalogInstallTasks(),
    queryFn: () => listSkillCatalogInstallTasks(commandClient),
    refetchInterval: (query) =>
      query.state.data?.tasks.some((task) => task.status === 'running') ? 1000 : false,
  })
}

export function useInstallSkillFromCatalog() {
  const commandClient = useCommandClient()

  return useMutation({
    mutationFn: ({ entry, operationId }: CatalogInstallMutationRequest) =>
      installSkillFromCatalog(
        {
          entryId: entry.entryId,
          operationId,
          sourceId: entry.sourceId,
          version: entry.version,
        },
        commandClient,
      ),
  })
}
