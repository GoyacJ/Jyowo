import '@testing-library/jest-dom/vitest'

import { fireEvent, screen, render as testingLibraryRender, waitFor } from '@testing-library/react'
import { I18nextProvider } from 'react-i18next'
import { describe, expect, it, vi } from 'vitest'
import type {
  CommandMetadata,
  PendingQuestionProjection,
  TypedUlid,
} from '@/generated/daemon-protocol'
import { createAppI18n } from '@/shared/i18n/i18n'
import { PendingQuestionForm } from './PendingQuestionForm'
import type { TaskCommandExecutor } from './use-task-command-executor'

const taskId = '01J00000000000000000000000' as TypedUlid

describe('PendingQuestionForm', () => {
  it('submits selected answers through resolve_question', async () => {
    let submitted: unknown
    const executeCommand = vi.fn(async (_operation, buildRequest) => {
      submitted = buildRequest(metadata())
      return {
        message: {
          commandId: metadata().commandId,
          committedOffset: 8,
          streamVersion: 4,
          taskId,
          type: 'command_accepted' as const,
        },
        protocolVersion: 7,
        requestId: 'question-test',
      }
    }) as TaskCommandExecutor
    render(
      <PendingQuestionForm
        executeCommand={executeCommand}
        pending={pendingQuestion()}
        taskId={taskId}
      />,
    )

    expect(screen.getByRole('button', { name: 'Submit answer' })).toBeDisabled()
    fireEvent.click(screen.getByRole('radio', { name: 'A' }))
    fireEvent.click(screen.getByRole('button', { name: 'Submit answer' }))

    await waitFor(() => expect(executeCommand).toHaveBeenCalledOnce())
    expect(submitted).toMatchObject({
      questionRequestId: pendingQuestion().requestId,
      requestRevision: 1,
      response: {
        answers: [
          {
            questionId: 'choice',
            selectedOptionIds: ['a'],
          },
        ],
        status: 'answered',
      },
      taskId,
      type: 'resolve_question',
    })
  })

  it('collects multi-select and free-text answers', async () => {
    let submitted: unknown
    const executeCommand = executor((request) => {
      submitted = request
    })
    const pending = pendingQuestion()
    pending.questions = [
      { ...pending.questions[0], multiSelect: true },
      {
        allowCustom: true,
        id: 'details',
        multiSelect: false,
        options: [],
        question: 'Add details',
      },
    ]
    render(
      <PendingQuestionForm executeCommand={executeCommand} pending={pending} taskId={taskId} />,
    )

    fireEvent.click(screen.getByRole('checkbox', { name: 'A' }))
    fireEvent.click(screen.getByRole('checkbox', { name: 'B' }))
    fireEvent.change(screen.getByRole('textbox', { name: 'Answer for Add details' }), {
      target: { value: 'Use the safer path' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Submit answer' }))

    await waitFor(() => expect(executeCommand).toHaveBeenCalledOnce())
    expect(submitted).toMatchObject({
      response: {
        answers: [
          { questionId: 'choice', selectedOptionIds: ['a', 'b'] },
          { questionId: 'details', selectedOptionIds: [], text: 'Use the safer path' },
        ],
        status: 'answered',
      },
    })
  })

  it('reveals and submits a custom option only when selected', async () => {
    let submitted: unknown
    const executeCommand = executor((request) => {
      submitted = request
    })
    const pending = pendingQuestion()
    pending.questions[0] = { ...pending.questions[0], allowCustom: true }
    render(
      <PendingQuestionForm executeCommand={executeCommand} pending={pending} taskId={taskId} />,
    )

    expect(screen.queryByRole('textbox')).not.toBeInTheDocument()
    fireEvent.click(screen.getByRole('radio', { name: /Other/ }))
    expect(screen.getByRole('button', { name: 'Submit answer' })).toBeDisabled()

    fireEvent.change(screen.getByRole('textbox', { name: 'Custom answer for Pick one' }), {
      target: { value: 'A safer alternative' },
    })
    fireEvent.click(screen.getByRole('button', { name: 'Submit answer' }))

    await waitFor(() => expect(executeCommand).toHaveBeenCalledOnce())
    expect(submitted).toMatchObject({
      response: {
        answers: [
          {
            questionId: 'choice',
            selectedOptionIds: [],
            text: 'A safer alternative',
          },
        ],
        status: 'answered',
      },
    })
  })

  it('keeps actions outside the scrollable question region and explains completion', () => {
    render(
      <PendingQuestionForm
        executeCommand={executor(() => undefined)}
        pending={pendingQuestion()}
        taskId={taskId}
      />,
    )

    const questions = screen.getByRole('region', { name: 'Questions' })
    const actions = screen.getByTestId('question-actions')
    expect(questions).toHaveClass('overflow-y-auto')
    expect(questions).not.toContainElement(actions)
    expect(screen.getByText('0 of 1 answered')).toBeInTheDocument()
    expect(screen.getByText('1 question left')).toBeInTheDocument()
  })

  it('lets the user decline the whole request', async () => {
    let submitted: unknown
    const executeCommand = executor((request) => {
      submitted = request
    })
    render(
      <PendingQuestionForm
        executeCommand={executeCommand}
        pending={pendingQuestion()}
        taskId={taskId}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: 'Skip for now' }))

    await waitFor(() => expect(executeCommand).toHaveBeenCalledOnce())
    expect(submitted).toMatchObject({ response: { status: 'declined' } })
  })
})

function render(ui: React.ReactNode) {
  const i18n = createAppI18n('en-US')
  return testingLibraryRender(ui, {
    wrapper: ({ children }) => <I18nextProvider i18n={i18n}>{children}</I18nextProvider>,
  })
}

function metadata(): CommandMetadata {
  return {
    commandId: '01J00000000000000000000001' as TypedUlid,
    expectedStreamVersion: 3,
    idempotencyKey: 'question-test',
  }
}

function executor(onRequest: (request: unknown) => void): TaskCommandExecutor {
  return vi.fn(async (_operation, buildRequest) => {
    onRequest(buildRequest(metadata()))
    return {
      message: {
        commandId: metadata().commandId,
        committedOffset: 8,
        streamVersion: 4,
        taskId,
        type: 'command_accepted' as const,
      },
      protocolVersion: 7,
      requestId: 'question-test',
    }
  }) as TaskCommandExecutor
}

function pendingQuestion(): PendingQuestionProjection {
  return {
    expiresAt: '2026-07-18T01:00:00Z',
    questions: [
      {
        allowCustom: false,
        id: 'choice',
        multiSelect: false,
        options: [
          { id: 'a', label: 'A' },
          { id: 'b', label: 'B' },
        ],
        question: 'Pick one',
      },
    ],
    requestId: '01J00000000000000000000002' as TypedUlid,
    revision: 1,
    segmentId: '01J00000000000000000000003' as TypedUlid,
    toolUseId: '01J00000000000000000000004' as TypedUlid,
  }
}
