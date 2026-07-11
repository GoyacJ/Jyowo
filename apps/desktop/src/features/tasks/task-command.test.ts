import { describe, expect, it } from 'vitest'

import { createTaskCommandMetadata } from './task-command'

describe('task command metadata', () => {
  it('keeps idempotency keys bounded when an operation includes message content', () => {
    const metadata = createTaskCommandMetadata(
      '01J00000000000000000000000',
      9,
      `submit:${'x'.repeat(64 * 1024)}`,
    )

    expect(new TextEncoder().encode(metadata.idempotencyKey).byteLength).toBeLessThanOrEqual(256)
  })
})
