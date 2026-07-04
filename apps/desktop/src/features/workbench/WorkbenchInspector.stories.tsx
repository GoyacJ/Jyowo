import { useEffect } from 'react'
import { WorkbenchInspector } from './WorkbenchInspector'
import { uiStore } from '@/shared/state/ui-store'
import type { WorkbenchSelection } from '@/shared/state/workbench-selection'

function WithSelection({ selection }: { selection: WorkbenchSelection | null }) {
  useEffect(() => {
    uiStore.setState({ inspectorOpen: true, workbenchSelection: selection })
    return () => {
      uiStore.setState({ inspectorOpen: false, workbenchSelection: null })
    }
  }, [selection])
  return <WorkbenchInspector />
}

export default {
  component: WorkbenchInspector,
  title: 'Features/Workbench/WorkbenchInspector',
}

export const Empty = {
  render: () => <WithSelection selection={null} />,
}

export const Context = {
  render: () => <WithSelection selection={{ kind: 'context' }} />,
}

export const Decision = {
  render: () => (
    <WithSelection
      selection={{ kind: 'decision', conversationId: 'conv-1', requestId: 'req-1' }}
    />
  ),
}

export const Tool = {
  render: () => (
    <WithSelection selection={{ kind: 'tool', conversationId: 'conv-1', toolUseId: 'tool-1' }} />
  ),
}

export const Command = {
  render: () => <WithSelection selection={{ kind: 'command', conversationId: 'conv-1' }} />,
}

export const Diff = {
  render: () => (
    <WithSelection
      selection={{ kind: 'diff', conversationId: 'conv-1', changeSetId: 'cs-1' }}
    />
  ),
}

export const Artifact = {
  render: () => (
    <WithSelection
      selection={{
        kind: 'artifact',
        conversationId: 'conv-1',
        artifactId: 'artifact-1',
        revisionId: 'rev-1',
      }}
    />
  ),
}
