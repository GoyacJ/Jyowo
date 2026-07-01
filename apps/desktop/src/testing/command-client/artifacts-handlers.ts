import {
  fixtureArtifactMediaPreview,
  fixtureAttachment,
  fixtureContextSnapshot,
  fixtureListArtifacts,
  fixtureReferenceCandidates,
  fixtureReplayTimeline,
  fixtureSupportBundleExport,
} from './artifacts'
import { wait } from './base'
import type { TestCommandClientState, TestCommandHandlers } from './state'

type ArtifactCommandKeys =
  | 'createAttachmentFromPath'
  | 'exportSupportBundle'
  | 'getArtifactMediaPreview'
  | 'getAttachmentMediaPreview'
  | 'getContextSnapshot'
  | 'getReplayTimeline'
  | 'listArtifacts'
  | 'listReferenceCandidates'

export function createArtifactCommandHandlers(
  state: TestCommandClientState,
): TestCommandHandlers<ArtifactCommandKeys> {
  return {
    async createAttachmentFromPath() {
      await wait(state.options.delayMs)
      return state.options.attachmentFromPath ?? fixtureAttachment
    },
    async exportSupportBundle() {
      await wait(state.options.delayMs)
      return state.options.supportBundleExport ?? fixtureSupportBundleExport
    },
    async getArtifactMediaPreview() {
      await wait(state.options.delayMs)
      return state.options.artifactMediaPreview ?? fixtureArtifactMediaPreview
    },
    async getAttachmentMediaPreview() {
      await wait(state.options.delayMs)
      if (state.options.attachmentMediaPreview) {
        return state.options.attachmentMediaPreview
      }
      throw new Error('attachment media preview is unavailable')
    },
    async getContextSnapshot() {
      await wait(state.options.delayMs)
      return state.options.contextSnapshot ?? fixtureContextSnapshot
    },
    async getReplayTimeline() {
      await wait(state.options.delayMs)
      return state.options.replayTimeline ?? fixtureReplayTimeline
    },
    async listArtifacts(_request) {
      await wait(state.options.delayMs)
      return state.options.artifacts ?? fixtureListArtifacts
    },
    async listReferenceCandidates(_request) {
      await wait(state.options.delayMs)
      return state.options.referenceCandidates ?? fixtureReferenceCandidates
    },
  }
}
