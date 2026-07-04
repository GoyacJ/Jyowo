import { describe, expect, it } from 'vitest'
import { createTestCommandClient } from './index'

describe('conversation command test client evidence handlers', () => {
  it('allows evidence and artifact responses to vary by request', async () => {
    const client = createTestCommandClient({
      artifactRevisionContent: (request) => ({
        artifactId: 'artifact-001',
        byteLength: request.contentRef.length,
        content: `artifact:${request.contentRef}`,
        contentType: 'text/plain; charset=utf-8',
        redactionState: 'clean',
        revisionId: 'revision-001',
        truncated: false,
      }),
      conversationCommandOutput: (request) => ({
        byteLength: request.fullOutputRef.length,
        contentType: 'text/plain; charset=utf-8',
        output: `output:${request.fullOutputRef}`,
        redactionState: 'clean',
        truncated: false,
      }),
      conversationDiffPatch: (request) => ({
        byteLength: request.fullPatchRef.length,
        contentType: 'text/x-diff; charset=utf-8',
        patch: `patch:${request.fullPatchRef}`,
        redactionState: 'clean',
        truncated: false,
      }),
    })

    await expect(
      client.getConversationCommandOutput({
        conversationId: 'conversation-001',
        fullOutputRef: 'output-ref-001',
      }),
    ).resolves.toMatchObject({ output: 'output:output-ref-001' })
    await expect(
      client.getConversationDiffPatch({
        conversationId: 'conversation-001',
        fullPatchRef: 'patch-ref-001',
      }),
    ).resolves.toMatchObject({ patch: 'patch:patch-ref-001' })
    await expect(
      client.getArtifactRevisionContent({
        conversationId: 'conversation-001',
        contentRef: 'content-ref-001',
      }),
    ).resolves.toMatchObject({ content: 'artifact:content-ref-001' })
  })
})
