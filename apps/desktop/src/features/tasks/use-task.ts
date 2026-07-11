import { useMemo } from 'react'
import { useStore } from 'zustand'

import type { TypedUlid } from '@/generated/daemon-protocol'

import { createTaskStore } from './task-store'
import { useTaskEvents } from './use-task-events'

const taskStores = new Map<TypedUlid, ReturnType<typeof createTaskStore>>()

export function useTask(taskId: TypedUlid) {
  const store = useMemo(() => taskStoreFor(taskId), [taskId])
  useTaskEvents(taskId, store)
  return useStore(store)
}

export function taskStoreFor(taskId: TypedUlid) {
  const existing = taskStores.get(taskId)
  if (existing) return existing
  const store = createTaskStore(taskId)
  taskStores.set(taskId, store)
  return store
}
