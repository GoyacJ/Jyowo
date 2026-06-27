import type {
  ConversationCursor,
  ConversationTurn,
  ConversationTurnCursor,
} from '@/shared/tauri/commands'
import type { ConversationTimelineAction } from './conversation-timeline-actions'

const redactedOptimisticBody = '[REDACTED]'
const optimisticSecretPatterns = [
  /\bAuthorization:?\s*Bearer\s+\S+/i,
  /\bAuthorization:?\s*Basic\s+[A-Za-z0-9+/=]{12,}/i,
  /\bBearer\s+\S+/i,
  /\bBasic\s+[A-Za-z0-9+/=]{12,}/i,
  /\bclient_secret\b\s*(?:=|:|\s+)\s*\S+/i,
  /\b(?:api[_-]?key|token|secret|password)\b\s*(?:=|:|\s+)\s*\S+/i,
  /\b--(?:api-key|token|secret|password)\b\s+\S+/i,
  /\b[A-Za-z0-9_]*(?:API_KEY|TOKEN|SECRET|PASSWORD|ACCESS_KEY)[A-Za-z0-9_]*\s*(?:=|\s+)\s*\S+/i,
  /\bsk-[A-Za-z0-9]{12,}/i,
  /\bgh[pousr]_[A-Za-z0-9_]{20,}/i,
  /\bgithub_pat_[A-Za-z0-9_]{20,}/i,
  /\bAKIA[0-9A-Z]{16}\b/,
  /\bAIza[0-9A-Za-z_-]{30,}\b/,
  /\bxox[baprs]-[0-9A-Za-z-]{20,}\b/,
  /\b(?:rk|sk)_(?:live|test)_[0-9A-Za-z]{12,}\b/i,
  /\bnpm_[0-9A-Za-z]{20,}\b/,
  /\blin_api_[0-9A-Za-z]{20,}\b/,
  /\bsecret_[0-9A-Za-z]{20,}\b/,
  /\beyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{6,}\b/,
]

export type ConversationTimelineState = {
  conversationId: string
  turns: ConversationTurn[]
  pageCursor: ConversationTurnCursor | null
  eventCursor: ConversationCursor | null
  hasMoreBefore: boolean
  hasMoreAfter: boolean
  gap: boolean
  activeRunIds: string[]
  refreshRequests: number
  immediateRefreshRequests: number
}

export type ConversationTimelineRoot = {
  byConversationId: Record<string, ConversationTimelineState>
}

export function createConversationTimelineRoot(): ConversationTimelineRoot {
  return { byConversationId: {} }
}

export function createConversationTimelineState(conversationId: string): ConversationTimelineState {
  return {
    conversationId,
    turns: [],
    pageCursor: null,
    eventCursor: null,
    hasMoreBefore: false,
    hasMoreAfter: false,
    gap: false,
    activeRunIds: [],
    refreshRequests: 0,
    immediateRefreshRequests: 0,
  }
}

export function getConversationTimelineState(
  root: ConversationTimelineRoot,
  conversationId: string,
): ConversationTimelineState {
  return root.byConversationId[conversationId] ?? createConversationTimelineState(conversationId)
}

export type ConversationTimelineRootAction = {
  conversationId: string
  action: ConversationTimelineAction
}

export function conversationTimelineRootReducerFromAction(
  root: ConversationTimelineRoot,
  scoped: ConversationTimelineRootAction,
): ConversationTimelineRoot {
  const current = getConversationTimelineState(root, scoped.conversationId)
  const next = conversationTimelineReducer(current, scoped.action)

  if (next === current) {
    return root
  }

  return {
    byConversationId: {
      ...root.byConversationId,
      [scoped.conversationId]: next,
    },
  }
}

function conversationTimelineReducer(
  state: ConversationTimelineState,
  action: ConversationTimelineAction,
): ConversationTimelineState {
  switch (action.type) {
    case 'hydrateWorktree':
      return {
        ...state,
        turns: reconcileOptimisticTurns(action.page.turns, state.turns),
        pageCursor: action.page.pageCursor ?? null,
        eventCursor: action.page.eventCursor ?? null,
        hasMoreBefore: action.page.hasMoreBefore,
        hasMoreAfter: action.page.hasMoreAfter,
        gap: action.page.gap,
        activeRunIds: activeRunIds(action.page.turns),
      }
    case 'localSubmit':
      return {
        ...state,
        turns: [
          ...state.turns,
          {
            id: `turn:local:${action.clientMessageId}`,
            conversationId: state.conversationId,
            position: nextOptimisticPosition(state.turns),
            user: {
              id: `user:local:${action.clientMessageId}`,
              messageId: `local:${action.clientMessageId}`,
              clientMessageId: action.clientMessageId,
              body: uiSafeCanvasText(action.draft.prompt),
              timestamp: action.at,
            },
          },
        ],
      }
    case 'commandAccepted':
      return {
        ...state,
        turns: state.turns.map((turn) =>
          turn.user.clientMessageId === action.clientMessageId
            ? {
                ...turn,
                assistant: {
                  id: `assistant:${action.runId}`,
                  runId: action.runId,
                  status: 'running',
                  segments: [],
                },
              }
            : turn,
        ),
        activeRunIds: addUnique(state.activeRunIds, action.runId),
      }
    case 'commandFailed':
      return {
        ...state,
        turns: state.turns.map((turn) =>
          turn.user.clientMessageId === action.clientMessageId
            ? {
                ...turn,
                assistant: {
                  id: `assistant:local-failed:${action.clientMessageId}`,
                  runId: `local-failed:${action.clientMessageId}`,
                  status: 'failed',
                  segments: [
                    {
                      kind: 'error',
                      id: `segment:error:local:${action.clientMessageId}`,
                      order: 0,
                      body: uiSafeCanvasText(action.errorMessage),
                    },
                  ],
                },
              }
            : turn,
        ),
      }
    case 'permissionSubmitting':
      return patchPermission(state, action.requestId, { status: 'submitting' })
    case 'permissionSubmitFailed':
      return patchPermission(state, action.requestId, {
        status: 'failed',
        summary: uiSafeCanvasText(action.errorMessage),
      })
    case 'markGap':
      return { ...state, gap: true }
    case 'worktreeRefreshRequested':
      return {
        ...state,
        refreshRequests: state.refreshRequests + 1,
        immediateRefreshRequests: state.immediateRefreshRequests + (action.immediate ? 1 : 0),
      }
  }
}

function uiSafeCanvasText(value: string) {
  if (containsOptimisticSecret(value) || containsPrivateAbsolutePath(value)) {
    return redactedOptimisticBody
  }
  return value
}

function containsOptimisticSecret(value: string) {
  return optimisticSecretPatterns.some((pattern) => pattern.test(value))
}

function containsPrivateAbsolutePath(value: string) {
  return /(?:\/Users\/|\/home\/|\/private\/var\/|[A-Za-z]:[\\/])/.test(value)
}

function reconcileOptimisticTurns(
  projectedTurns: ConversationTurn[],
  currentTurns: ConversationTurn[],
) {
  const projectedClientMessageIds = new Set(
    projectedTurns.flatMap((turn) =>
      turn.user.clientMessageId ? [turn.user.clientMessageId] : [],
    ),
  )
  const projectedRunIds = new Set(
    projectedTurns.flatMap((turn) => (turn.assistant?.runId ? [turn.assistant.runId] : [])),
  )
  const optimistic = currentTurns.filter(
    (turn) =>
      turn.id.startsWith('turn:local:') &&
      !isProjectedOptimisticTurn(turn, projectedClientMessageIds, projectedRunIds),
  )

  return [...projectedTurns, ...optimistic]
}

function isProjectedOptimisticTurn(
  turn: ConversationTurn,
  projectedClientMessageIds: Set<string>,
  projectedRunIds: Set<string>,
) {
  if (turn.user.clientMessageId && projectedClientMessageIds.has(turn.user.clientMessageId)) {
    return true
  }

  return Boolean(turn.assistant?.runId && projectedRunIds.has(turn.assistant.runId))
}

function activeRunIds(turns: ConversationTurn[]) {
  return turns.flatMap((turn) =>
    turn.assistant?.status === 'running' ? [turn.assistant.runId] : [],
  )
}

function nextOptimisticPosition(turns: ConversationTurn[]) {
  return turns.reduce((max, turn) => Math.max(max, turn.position), -1) + 1
}

function addUnique(values: string[], value: string) {
  return values.includes(value) ? values : [...values, value]
}

function patchPermission(
  state: ConversationTimelineState,
  requestId: string,
  patch: { status: 'submitting' | 'failed'; summary?: string },
): ConversationTimelineState {
  return {
    ...state,
    turns: state.turns.map((turn) => ({
      ...turn,
      assistant: turn.assistant
        ? {
            ...turn.assistant,
            segments: turn.assistant.segments.map((segment) =>
              segment.kind === 'toolGroup'
                ? {
                    ...segment,
                    attempts: segment.attempts.map((attempt) =>
                      attempt.permission?.requestId === requestId
                        ? {
                            ...attempt,
                            permission: {
                              ...attempt.permission,
                              ...patch,
                            },
                          }
                        : attempt,
                    ),
                  }
                : segment,
            ),
          }
        : undefined,
    })),
  }
}
