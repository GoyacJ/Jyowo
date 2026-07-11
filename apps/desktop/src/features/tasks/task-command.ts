import type { CommandMetadata, ServerFrame, TypedUlid } from '@/generated/daemon-protocol'

const ULID_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'

export class TaskCommandError extends Error {
  constructor(readonly reason: string) {
    super(reason.replaceAll('_', ' '))
    this.name = 'TaskCommandError'
  }
}

export function createTaskCommandMetadata(
  taskId: TypedUlid,
  expectedStreamVersion: number,
  operation: string,
): CommandMetadata {
  const commandId = createUlid()
  return {
    commandId,
    expectedStreamVersion,
    idempotencyKey: `${taskId}:${operation}:${commandId}`,
  }
}

export function requireAcceptedCommand(frame: ServerFrame, taskId: TypedUlid) {
  if (frame.message.type === 'command_rejected') {
    throw new TaskCommandError(frame.message.reason)
  }
  if (frame.message.type === 'error') {
    throw new TaskCommandError(frame.message.message)
  }
  if (frame.message.type !== 'command_accepted') {
    throw new TaskCommandError(`unexpected_${frame.message.type}`)
  }
  if (frame.message.taskId !== taskId) {
    throw new TaskCommandError('response_task_mismatch')
  }
  return frame.message
}

function createUlid(): TypedUlid {
  const random = new Uint8Array(10)
  crypto.getRandomValues(random)

  let timestamp = BigInt(Date.now())
  let timePart = ''
  for (let index = 0; index < 10; index += 1) {
    timePart = ULID_ALPHABET[Number(timestamp & 31n)] + timePart
    timestamp >>= 5n
  }

  let randomness = 0n
  for (const byte of random) randomness = (randomness << 8n) | BigInt(byte)
  let randomPart = ''
  for (let index = 0; index < 16; index += 1) {
    randomPart = ULID_ALPHABET[Number(randomness & 31n)] + randomPart
    randomness >>= 5n
  }

  return `${timePart}${randomPart}`
}
