import type { ChangeSetFile } from '@/shared/tauri/commands'
import { DiffPane } from './DiffPane'

export default { component: DiffPane, title: 'Features/Evidence/DiffPane' }

const smallFiles: ChangeSetFile[] = [
  {
    path: 'src/main.rs',
    status: 'modified',
    addedLines: 5,
    removedLines: 2,
    preview: '+ fn main() {\n  println!("hello");\n}',
  },
  {
    path: 'src/lib.rs',
    status: 'modified',
    addedLines: 3,
    removedLines: 1,
    preview: '+ pub fn add(a: i32, b: i32) -> i32 { a + b }',
  },
]
const largeFile: ChangeSetFile[] = [
  {
    path: 'src/big.rs',
    status: 'modified',
    addedLines: 5000,
    removedLines: 3000,
    riskFlags: ['large'],
    preview: '...',
  },
]
const binaryFile: ChangeSetFile[] = [
  { path: 'asset.png', status: 'added', addedLines: 0, removedLines: 0, riskFlags: ['binary'] },
]
const generatedFile: ChangeSetFile[] = [
  {
    path: 'gen.rs',
    status: 'modified',
    addedLines: 100,
    removedLines: 50,
    riskFlags: ['generated'],
  },
]

export const Small = { render: () => <DiffPane conversationId="c1" files={smallFiles} /> }
export const Large = { render: () => <DiffPane conversationId="c1" files={largeFile} /> }
export const Binary = { render: () => <DiffPane conversationId="c1" files={binaryFile} /> }
export const Generated = { render: () => <DiffPane conversationId="c1" files={generatedFile} /> }
export const MultipleRevisions = {
  render: () => (
    <DiffPane conversationId="c1" files={[...smallFiles, ...largeFile, ...binaryFile]} />
  ),
}
