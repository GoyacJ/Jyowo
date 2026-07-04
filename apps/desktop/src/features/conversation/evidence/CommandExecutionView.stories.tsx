import type { CommandExecution } from '@/shared/tauri/commands'
import { CommandExecutionView } from './CommandExecutionView'

function makeCmd(overrides?: Partial<CommandExecution>): CommandExecution {
  return {
    command: 'cargo test',
    truncated: true,
    redactionState: 'clean',
    riskLevel: 'low',
    ...overrides,
  }
}

export default { component: CommandExecutionView, title: 'Features/Evidence/CommandExecutionView' }

export const Clean = {
  render: () => (
    <CommandExecutionView
      command={makeCmd({
        exitCode: 0,
        durationMs: 1200,
        stdoutPreview: 'test result: ok. 5 passed; 0 failed',
      })}
      conversationId="c1"
    />
  ),
}
export const Redacted = {
  render: () => (
    <CommandExecutionView
      command={makeCmd({ redactionState: 'redacted', stdoutPreview: 'result: ok' })}
      conversationId="c1"
    />
  ),
}
export const Withheld = {
  render: () => (
    <CommandExecutionView command={makeCmd({ redactionState: 'withheld' })} conversationId="c1" />
  ),
}
export const Failed = {
  render: () => (
    <CommandExecutionView
      command={makeCmd({ exitCode: 1, stderrPreview: 'error: compilation failed' })}
      conversationId="c1"
    />
  ),
}
