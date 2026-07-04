import { useUiStore } from '@/shared/state/ui-store'
import type { WorkbenchSelection } from '@/shared/state/workbench-selection'

export function useWorkbenchSelection() {
  return useUiStore((state) => state.workbenchSelection)
}

export function useSetWorkbenchSelection() {
  return useUiStore((state) => state.setWorkbenchSelection)
}

export function useSelectEvidence() {
  const setSelection = useSetWorkbenchSelection()
  return (selection: WorkbenchSelection) => {
    setSelection(selection)
  }
}

export function useCloseInspector() {
  const setSelection = useSetWorkbenchSelection()
  return () => {
    setSelection(null)
  }
}
