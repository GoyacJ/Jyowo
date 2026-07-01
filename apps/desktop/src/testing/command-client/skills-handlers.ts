import type { GetSkillDetailResponse } from '@/shared/tauri/commands'

import { wait } from './base'
import {
  fixtureListSkills,
  fixtureSkillCatalogEntries,
  fixtureSkillCatalogEntry,
  fixtureSkillCatalogFile,
  fixtureSkillCatalogInstallTasks,
  fixtureSkillCatalogSources,
  fixtureSkillDetail,
  fixtureSkillEntryFile,
  fixtureWorkspaceSkill,
} from './skills'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type SkillCommandKeys =
  | 'deleteSkill'
  | 'getSkillCatalogEntry'
  | 'getSkillCatalogFile'
  | 'getSkillDetail'
  | 'getSkillFile'
  | 'importSkill'
  | 'installSkillFromCatalog'
  | 'listSkillCatalogEntries'
  | 'listSkillCatalogInstallTasks'
  | 'listSkillCatalogSources'
  | 'listSkills'
  | 'listenSkillCatalogInstallProgress'
  | 'setSkillEnabled'

export function createSkillCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<SkillCommandKeys> {
  return {
    async deleteSkill(id) {
      await wait(state.options.delayMs)
      return { id, status: 'deleted' }
    },
    async getSkillCatalogEntry() {
      await wait(state.options.delayMs)
      return state.options.skillCatalogEntry ?? fixtureSkillCatalogEntry
    },
    async getSkillCatalogFile() {
      await wait(state.options.delayMs)
      return state.options.skillCatalogFile ?? fixtureSkillCatalogFile
    },
    async getSkillDetail(id) {
      await wait(state.options.delayMs)
      if (state.options.skillDetail) {
        return state.options.skillDetail
      }

      const summary =
        (state.options.skills ?? fixtureListSkills).skills.find((skill) => skill.id === id) ??
        fixtureWorkspaceSkill

      return {
        skill: {
          ...fixtureSkillDetail.skill,
          summary,
        },
      } satisfies GetSkillDetailResponse
    },
    async getSkillFile(_id, path) {
      await wait(state.options.delayMs)
      if (state.options.skillFile) {
        return state.options.skillFile
      }

      return path === fixtureSkillEntryFile.file.path
        ? fixtureSkillEntryFile
        : {
            file: {
              content: `Fixture content for ${path}`,
              path,
            },
          }
    },
    async importSkill() {
      await wait(state.options.delayMs)
      return { skill: fixtureWorkspaceSkill }
    },
    async installSkillFromCatalog(request) {
      state.emitCatalogInstallProgress(request, 'preparing', 5)
      await wait(state.options.delayMs)
      state.emitCatalogInstallProgress(request, 'completed', 100)
      return (
        state.options.skillCatalogInstall ?? {
          task: {
            entryId: request.entryId,
            operationId: request.operationId ?? 'catalog-install-fixture',
            percent: 5,
            sourceId: request.sourceId,
            stage: 'preparing',
            startedAt: '2026-06-28T00:00:00Z',
            status: 'running',
            updatedAt: '2026-06-28T00:00:00Z',
            version: request.version,
          },
        }
      )
    },
    async listSkillCatalogEntries() {
      await wait(state.options.delayMs)
      return state.options.skillCatalogEntries ?? fixtureSkillCatalogEntries
    },
    async listSkillCatalogInstallTasks() {
      await wait(state.options.delayMs)
      return state.options.skillCatalogInstallTasks ?? fixtureSkillCatalogInstallTasks
    },
    async listSkillCatalogSources() {
      await wait(state.options.delayMs)
      return state.options.skillCatalogSources ?? fixtureSkillCatalogSources
    },
    async listSkills() {
      await wait(state.options.delayMs)
      return state.options.skills ?? fixtureListSkills
    },
    async listenSkillCatalogInstallProgress(onProgress) {
      state.catalogInstallProgressListeners.add(onProgress)
      return () => {
        state.catalogInstallProgressListeners.delete(onProgress)
      }
    },
    async setSkillEnabled(id, enabled) {
      await wait(state.options.delayMs)
      const skill =
        (state.options.skills ?? fixtureListSkills).skills.find(
          (currentSkill) => currentSkill.id === id,
        ) ?? fixtureWorkspaceSkill

      return {
        skill: {
          ...skill,
          enabled,
          status: enabled ? 'ready' : 'disabled',
        },
      }
    },
  }
}
