import type {
  ArtifactRevisionSummary,
  AssistantWork,
  ChangeSetFile,
  DecisionRequestState,
  ProcessStep,
  RunModelSnapshot,
} from '@/shared/tauri/commands'

export const fixtureRunModelSnapshot: RunModelSnapshot = {
  modelConfigId: 'provider-config-001',
  providerId: 'openai',
  modelId: 'gpt-4.1',
  displayName: 'GPT-4.1',
  protocol: 'responses',
}

export function assistantWork(
  input: Omit<AssistantWork, 'projectionVersion'> & {
    projectionVersion?: number
  },
): AssistantWork {
  return {
    projectionVersion: 1,
    ...input,
  }
}

export function permissionState(
  input: Pick<DecisionRequestState, 'id' | 'requestId' | 'status'> &
    Partial<Omit<DecisionRequestState, 'id' | 'requestId' | 'status'>>,
): DecisionRequestState {
  return {
    operation: 'read',
    target: {
      kind: 'file',
      label: 'workspace file',
    },
    riskLevel: 'low',
    reason: 'Read workspace context',
    policy: {
      mode: 'default',
    },
    decisionOptions: [
      {
        id: 'option-allow-once',
        decision: 'approve',
        label: 'Allow once',
        lifetime: 'once',
        matcher: {
          kind: 'toolName',
          label: 'Current tool',
        },
        requiresConfirmation: false,
      },
      {
        id: 'option-deny-once',
        decision: 'deny',
        label: 'Deny once',
        lifetime: 'once',
        matcher: {
          kind: 'toolName',
          label: 'Current tool',
        },
        requiresConfirmation: false,
      },
    ],
    dataExposure: {
      sendsWorkspaceData: true,
      sendsNetworkData: false,
      touchesPrivatePath: false,
      secretRisk: 'none',
    },
    ...input,
  }
}

export function artifactRevision(
  input: Pick<
    ArtifactRevisionSummary,
    'artifactId' | 'revisionId' | 'kind' | 'sourceRunId' | 'title'
  > &
    Partial<
      Omit<ArtifactRevisionSummary, 'artifactId' | 'revisionId' | 'kind' | 'sourceRunId' | 'title'>
    >,
): ArtifactRevisionSummary {
  return {
    status: 'ready',
    ...input,
  }
}

export function changeSetFile(
  input: Pick<ChangeSetFile, 'path' | 'addedLines' | 'removedLines'> &
    Partial<Omit<ChangeSetFile, 'path' | 'addedLines' | 'removedLines'>>,
): ChangeSetFile {
  return {
    status: 'modified',
    ...input,
  }
}

export function commandDetail(
  input: Pick<Extract<NonNullable<ProcessStep['detail']>, { type: 'command' }>, 'command'> &
    Partial<
      Omit<Extract<NonNullable<ProcessStep['detail']>, { type: 'command' }>, 'type' | 'command'>
    >,
): Extract<NonNullable<ProcessStep['detail']>, { type: 'command' }> {
  return {
    type: 'command',
    truncated: false,
    redactionState: 'clean',
    riskLevel: 'low',
    ...input,
  }
}

export function diffDetail(
  input: Pick<
    Extract<NonNullable<ProcessStep['detail']>, { type: 'diff' }>,
    'id' | 'summary' | 'files'
  >,
): Extract<NonNullable<ProcessStep['detail']>, { type: 'diff' }> {
  return {
    type: 'diff',
    ...input,
  }
}
