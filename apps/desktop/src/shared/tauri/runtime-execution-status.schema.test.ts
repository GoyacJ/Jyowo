import { describe, expect, it } from 'vitest'
import { runtimeExecutionStatusSchema } from './commands'

describe('runtime execution status schema', () => {
  const validPayload = {
    processSandbox: {
      backendId: 'routing',
      candidateIds: ['local', 'docker', 'local_none'],
      availableNetworkPolicies: ['none', 'unrestricted'],
      availableWorkspacePolicies: ['read_write_all'],
      unavailableReasons: [],
    },
    httpBroker: {
      available: true,
      deniedReasons: [],
    },
    tools: [
      {
        toolName: 'Bash',
        available: true,
        unavailableReason: null,
      },
      {
        toolName: 'MiniMaxTextToImage',
        available: true,
        unavailableReason: null,
      },
      {
        toolName: 'SendMessage',
        available: false,
        unavailableReason: 'UserMessenger capability is not registered',
      },
    ],
  }

  it('accepts a valid full payload', () => {
    const result = runtimeExecutionStatusSchema.safeParse(validPayload)
    expect(result.success).toBe(true)
  })

  it('rejects payload with missing processSandbox', () => {
    const { processSandbox: _, ...rest } = validPayload
    const result = runtimeExecutionStatusSchema.safeParse(rest)
    expect(result.success).toBe(false)
  })

  it('rejects payload with missing httpBroker', () => {
    const { httpBroker: _, ...rest } = validPayload
    const result = runtimeExecutionStatusSchema.safeParse(rest)
    expect(result.success).toBe(false)
  })

  it('rejects payload with missing tools array', () => {
    const { tools: _, ...rest } = validPayload
    const result = runtimeExecutionStatusSchema.safeParse(rest)
    expect(result.success).toBe(false)
  })

  it('rejects tool with empty toolName', () => {
    const invalid = {
      ...validPayload,
      tools: [{ toolName: '', available: true, unavailableReason: null }],
    }
    const result = runtimeExecutionStatusSchema.safeParse(invalid)
    expect(result.success).toBe(false)
  })

  it('rejects tool with non-boolean available', () => {
    const invalid = {
      ...validPayload,
      tools: [{ toolName: 'Bash', available: 'yes', unavailableReason: null }],
    }
    const result = runtimeExecutionStatusSchema.safeParse(invalid)
    expect(result.success).toBe(false)
  })

  it('rejects broker with non-boolean available', () => {
    const invalid = {
      ...validPayload,
      httpBroker: { available: 'yes', deniedReasons: [] },
    }
    const result = runtimeExecutionStatusSchema.safeParse(invalid)
    expect(result.success).toBe(false)
  })

  it('rejects empty backendId', () => {
    const invalid = {
      ...validPayload,
      processSandbox: { ...validPayload.processSandbox, backendId: '' },
    }
    const result = runtimeExecutionStatusSchema.safeParse(invalid)
    expect(result.success).toBe(false)
  })
})
