import { describe, expect, it } from 'vitest'
import { createTestCommandClient } from './index'

const validEvidenceContentHash = 'e'.repeat(64)

describe('conversation command test client evidence handlers', () => {
  it('allows evidence and artifact responses to vary by request', async () => {
    const client = createTestCommandClient({
      artifactRevisionContent: (request) => ({
        artifactId: 'artifact-001',
        byteLength: request.contentRef.length,
        content: `artifact:${request.contentRef}`,
        contentHash: validEvidenceContentHash,
        contentBytes: request.contentRef.length,
        contentType: 'text/plain; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'artifact-content',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        redactionState: 'clean',
        refId: request.contentRef,
        returnedBytes: request.contentRef.length,
        revisionId: 'revision-001',
        totalBytes: request.contentRef.length,
        truncated: false,
      }),
      conversationCommandOutput: (request) => ({
        byteLength: request.fullOutputRef.length,
        contentHash: validEvidenceContentHash,
        contentBytes: request.fullOutputRef.length,
        contentType: 'text/plain; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'command-output',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        output: `output:${request.fullOutputRef}`,
        redactionState: 'clean',
        refId: request.fullOutputRef,
        returnedBytes: request.fullOutputRef.length,
        totalBytes: request.fullOutputRef.length,
        truncated: false,
      }),
      conversationDiffPatch: (request) => ({
        byteLength: request.fullPatchRef.length,
        contentHash: validEvidenceContentHash,
        contentBytes: request.fullPatchRef.length,
        contentType: 'text/x-diff; charset=utf-8',
        hasMore: false,
        hashAlgorithm: 'blake3',
        kind: 'diff-patch',
        limitBytes: 65_536,
        maxBytes: 65_536,
        offsetBytes: 0,
        patch: `patch:${request.fullPatchRef}`,
        redactionState: 'clean',
        refId: request.fullPatchRef,
        returnedBytes: request.fullPatchRef.length,
        totalBytes: request.fullPatchRef.length,
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
