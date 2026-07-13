import type { CommandMetadata, ServerFrame, TypedUlid } from '@/generated/daemon-protocol'

const ULID_ALPHABET = '0123456789ABCDEFGHJKMNPQRSTVWXYZ'

class TaskCommandError extends Error {
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
    idempotencyKey: `${taskId}:${boundedOperation(operation)}:${commandId}`,
  }
}

function boundedOperation(operation: string) {
  const prefix = (operation.split(':', 1)[0] || 'command')
    .replaceAll(/[^a-zA-Z0-9_-]/g, '_')
    .slice(0, 24)
  let first = 0x811c9dc5
  let second = 0x9e3779b9
  for (let index = 0; index < operation.length; index += 1) {
    const code = operation.charCodeAt(index)
    first = Math.imul(first ^ code, 0x01000193)
    second = Math.imul(second ^ code, 0x85ebca6b)
  }
  return `${prefix}:${(first >>> 0).toString(16).padStart(8, '0')}${(second >>> 0)
    .toString(16)
    .padStart(8, '0')}`
}

export function createTaskCreationMetadata(): CommandMetadata {
  const commandId = createUlid()
  return {
    commandId,
    expectedStreamVersion: 0,
    idempotencyKey: `create:${commandId}`,
  }
}

export function requireAcceptedCommand(frame: ServerFrame, taskId: TypedUlid) {
  if (frame.message.type === 'command_rejected') {
    throw new TaskCommandError(frame.message.message ?? frame.message.reason)
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
