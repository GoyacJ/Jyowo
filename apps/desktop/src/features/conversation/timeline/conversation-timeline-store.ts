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

type TimelinePage = {
  cursor: ConversationTurnCursor | null
  turns: ConversationTurn[]
}

export type ConversationTimelineState = {
  conversationId: string
  pages: TimelinePage[]
  loadedRange: {
    first?: ConversationTurnCursor
    last?: ConversationTurnCursor
  }
  hasMoreBefore: boolean
  hasMoreAfter: boolean
  gapMarkers: Array<{ id: string; afterCursor: ConversationCursor | null }>
  eventCursor: ConversationCursor | null
  optimisticTurnsByClientMessageId: Record<string, ConversationTurn>
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
    pages: [],
    loadedRange: {},
    hasMoreBefore: false,
    hasMoreAfter: false,
    gapMarkers: [],
    eventCursor: null,
    optimisticTurnsByClientMessageId: {},
    activeRunIds: [],
    refreshRequests: 0,
    immediateRefreshRequests: 0,
  }
}

export function getAllTurns(state: ConversationTimelineState): ConversationTurn[] {
  const projected = state.pages.flatMap((page) => page.turns)
  const optimistic = Object.values(state.optimisticTurnsByClientMessageId).filter(
    (turn) =>
      !projected.some(
        (p) =>
          (turn.user.clientMessageId && p.user.clientMessageId === turn.user.clientMessageId) ||
          (turn.assistant?.runId && p.assistant?.runId === turn.assistant.runId),
      ),
  )
  return [...projected, ...optimistic]
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
    case 'hydrateInitialPage': {
      const reconciled = reconcileOptimisticTurns(
        action.turns,
        Object.values(state.optimisticTurnsByClientMessageId),
      )
      return {
        ...state,
        pages: [{ cursor: action.pageCursor, turns: reconciled }],
        loadedRange: {
          first: action.pageCursor ?? undefined,
          last: action.pageCursor ?? undefined,
        },
        hasMoreBefore: action.hasMoreBefore,
        hasMoreAfter: action.hasMoreAfter,
        eventCursor: action.eventCursor,
        gapMarkers: action.gap
          ? [...state.gapMarkers, { id: `gap:${Date.now()}`, afterCursor: action.eventCursor }]
          : state.gapMarkers,
        activeRunIds: activeRunIds(reconciled),
        optimisticTurnsByClientMessageId: filterOptimisticAfterReconcile(
          state.optimisticTurnsByClientMessageId,
          reconciled,
        ),
      }
    }
    case 'prependPage': {
      if (action.turns.length === 0) {
        return { ...state, hasMoreBefore: action.hasMoreBefore }
      }
      const existingTurnIds = new Set(state.pages.flatMap((p) => p.turns.map((t) => t.id)))
      const newTurns = action.turns.filter((t) => !existingTurnIds.has(t.id))
      return {
        ...state,
        pages: [{ cursor: action.pageCursor, turns: newTurns }, ...state.pages],
        loadedRange: {
          ...state.loadedRange,
          first: action.pageCursor ?? state.loadedRange.first,
        },
        hasMoreBefore: action.hasMoreBefore,
      }
    }
    case 'appendPage': {
      if (action.turns.length === 0) {
        return {
          ...state,
          hasMoreAfter: action.hasMoreAfter,
          eventCursor: action.eventCursor ?? state.eventCursor,
        }
      }
      const existingTurnIds = new Set(state.pages.flatMap((p) => p.turns.map((t) => t.id)))
      const newTurns = action.turns.filter((t) => !existingTurnIds.has(t.id))
      return {
        ...state,
        pages: [...state.pages, { cursor: action.pageCursor, turns: newTurns }],
        loadedRange: {
          ...state.loadedRange,
          last: action.pageCursor ?? state.loadedRange.last,
        },
        hasMoreAfter: action.hasMoreAfter,
        eventCursor: action.eventCursor ?? state.eventCursor,
      }
    }
    case 'localSubmit':
      return {
        ...state,
        optimisticTurnsByClientMessageId: {
          ...state.optimisticTurnsByClientMessageId,
          [action.clientMessageId]: {
            id: `turn:local:${action.clientMessageId}`,
            conversationId: state.conversationId,
            position: nextOptimisticPosition(state),
            user: {
              id: `user:local:${action.clientMessageId}`,
              messageId: `local:${action.clientMessageId}`,
              clientMessageId: action.clientMessageId,
              body: uiSafeCanvasText(action.draft.prompt),
              timestamp: action.at,
            },
          },
        },
      }
    case 'commandAccepted':
      return {
        ...state,
        optimisticTurnsByClientMessageId: mapOptimisticTurn(
          state.optimisticTurnsByClientMessageId,
          action.clientMessageId,
          (turn) => ({
            ...turn,
            assistant: {
              id: `assistant:${action.runId}`,
              runId: action.runId,
              projectionVersion: 1,
              status: 'running' as const,
              segments: [],
            },
          }),
        ),
        pages: state.pages.map((page) => ({
          ...page,
          turns: page.turns.map((turn) =>
            turn.user.clientMessageId === action.clientMessageId
              ? {
                  ...turn,
                  assistant: {
                    id: `assistant:${action.runId}`,
                    runId: action.runId,
                    projectionVersion: 1,
                    status: 'running' as const,
                    segments: [],
                  },
                }
              : turn,
          ),
        })),
        activeRunIds: addUnique(state.activeRunIds, action.runId),
      }
    case 'commandFailed':
      return {
        ...state,
        optimisticTurnsByClientMessageId: mapOptimisticTurn(
          state.optimisticTurnsByClientMessageId,
          action.clientMessageId,
          (turn) => ({
            ...turn,
            assistant: {
              id: `assistant:local-failed:${action.clientMessageId}`,
              runId: `local-failed:${action.clientMessageId}`,
              projectionVersion: 1,
              status: 'failed' as const,
              segments: [
                {
                  kind: 'error' as const,
                  id: `segment:error:local:${action.clientMessageId}`,
                  order: 0,
                  body: uiSafeCanvasText(action.errorMessage),
                },
              ],
            },
          }),
        ),
        pages: state.pages.map((page) => ({
          ...page,
          turns: page.turns.map((turn) =>
            turn.user.clientMessageId === action.clientMessageId
              ? {
                  ...turn,
                  assistant: {
                    id: `assistant:local-failed:${action.clientMessageId}`,
                    runId: `local-failed:${action.clientMessageId}`,
                    projectionVersion: 1,
                    status: 'failed' as const,
                    segments: [
                      {
                        kind: 'error' as const,
                        id: `segment:error:local:${action.clientMessageId}`,
                        order: 0,
                        body: uiSafeCanvasText(action.errorMessage),
                      },
                    ],
                  },
                }
              : turn,
          ),
        })),
      }
    case 'permissionSubmitting':
      return patchPermission(state, action.requestId, { status: 'submitting' })
    case 'permissionSubmitFailed':
      return patchPermission(state, action.requestId, {
        status: 'failed',
        reason: uiSafeCanvasText(action.errorMessage),
      })
    case 'markGap':
      return {
        ...state,
        gapMarkers: [
          ...state.gapMarkers,
          { id: `gap:${Date.now()}`, afterCursor: action.afterCursor },
        ],
      }
    case 'retryGap':
      return {
        ...state,
        gapMarkers: [],
        refreshRequests: state.refreshRequests + 1,
        immediateRefreshRequests: state.immediateRefreshRequests + 1,
      }
    case 'worktreeRefreshRequested':
      return {
        ...state,
        refreshRequests: state.refreshRequests + 1,
        immediateRefreshRequests: state.immediateRefreshRequests + (action.immediate ? 1 : 0),
      }
  }
}

// ── Helpers ──

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
  optimisticTurns: ConversationTurn[],
): ConversationTurn[] {
  const projectedClientMessageIds = new Set(
    projectedTurns.flatMap((turn) =>
      turn.user.clientMessageId ? [turn.user.clientMessageId] : [],
    ),
  )
  const projectedRunIds = new Set(
    projectedTurns.flatMap((turn) => (turn.assistant?.runId ? [turn.assistant.runId] : [])),
  )
  const remaining = optimisticTurns.filter(
    (turn) =>
      !(
        (turn.user.clientMessageId && projectedClientMessageIds.has(turn.user.clientMessageId)) ||
        (turn.assistant?.runId && projectedRunIds.has(turn.assistant.runId))
      ),
  )
  return [...projectedTurns, ...remaining]
}

function filterOptimisticAfterReconcile(
  optimistic: Record<string, ConversationTurn>,
  projectedTurns: ConversationTurn[],
): Record<string, ConversationTurn> {
  const projectedIds = new Set(
    projectedTurns.flatMap((turn) =>
      turn.user.clientMessageId ? [turn.user.clientMessageId] : [],
    ),
  )
  const projectedRunIds = new Set(
    projectedTurns.flatMap((turn) => (turn.assistant?.runId ? [turn.assistant.runId] : [])),
  )
  const filtered: Record<string, ConversationTurn> = {}
  for (const [key, turn] of Object.entries(optimistic)) {
    if (
      (turn.user.clientMessageId && projectedIds.has(turn.user.clientMessageId)) ||
      (turn.assistant?.runId && projectedRunIds.has(turn.assistant.runId))
    ) {
      continue
    }
    filtered[key] = turn
  }
  return filtered
}

function mapOptimisticTurn(
  optimistic: Record<string, ConversationTurn>,
  clientMessageId: string,
  fn: (turn: ConversationTurn) => ConversationTurn,
): Record<string, ConversationTurn> {
  const existing = optimistic[clientMessageId]
  if (!existing) return optimistic
  return { ...optimistic, [clientMessageId]: fn(existing) }
}

function activeRunIds(turns: ConversationTurn[]) {
  return turns.flatMap((turn) =>
    turn.assistant?.status === 'running' ? [turn.assistant.runId] : [],
  )
}

function nextOptimisticPosition(state: ConversationTimelineState) {
  let max = 0
  for (const page of state.pages) {
    for (const turn of page.turns) {
      if (turn.position > max) max = turn.position
    }
  }
  for (const turn of Object.values(state.optimisticTurnsByClientMessageId)) {
    if (turn.position > max) max = turn.position
  }
  return max + 1
}

function addUnique(values: string[], value: string) {
  return values.includes(value) ? values : [...values, value]
}

function patchPermission(
  state: ConversationTimelineState,
  requestId: string,
  patch: { status: 'submitting' | 'failed'; reason?: string },
): ConversationTimelineState {
  const patchPage = (page: TimelinePage): TimelinePage => ({
    ...page,
    turns: page.turns.map(patchTurn),
  })

  const patchTurn = (turn: ConversationTurn): ConversationTurn => ({
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
  })

  return {
    ...state,
    pages: state.pages.map(patchPage),
    optimisticTurnsByClientMessageId: Object.fromEntries(
      Object.entries(state.optimisticTurnsByClientMessageId).map(([key, turn]) => [
        key,
        patchTurn(turn),
      ]),
    ),
  }
}
