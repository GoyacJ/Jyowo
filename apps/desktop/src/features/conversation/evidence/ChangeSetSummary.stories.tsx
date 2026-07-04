import type { ChangeSet } from '@/shared/tauri/commands'
import { ChangeSetSummary } from './ChangeSetSummary'

export default { component: ChangeSetSummary, title: 'Features/Evidence/ChangeSetSummary' }

const modified: ChangeSet = {
  id: 'cs-1',
  summary: 'Updated main.rs',
  files: [{ path: 'src/main.rs', status: 'modified', addedLines: 5, removedLines: 2 }],
}
const multiFile: ChangeSet = {
  id: 'cs-2',
  summary: 'Refactored 3 files',
  files: [
    { path: 'src/a.rs', status: 'modified', addedLines: 10, removedLines: 5 },
    { path: 'src/b.rs', status: 'added', addedLines: 20, removedLines: 0 },
    {
      path: 'src/old.rs',
      status: 'deleted',
      addedLines: 0,
      removedLines: 30,
      riskFlags: ['delete'],
    },
  ],
}
const risky: ChangeSet = {
  id: 'cs-3',
  summary: 'Binary asset update',
  files: [
    { path: 'logo.png', status: 'modified', addedLines: 0, removedLines: 0, riskFlags: ['binary'] },
  ],
}

export const SingleFile = { render: () => <ChangeSetSummary changeSet={modified} /> }
export const MultipleFiles = { render: () => <ChangeSetSummary changeSet={multiFile} /> }
export const WithRiskFlags = { render: () => <ChangeSetSummary changeSet={risky} /> }
