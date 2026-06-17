import type { ReactNode } from 'react'

const primaryNavigationItems = [
  'Workspaces',
  'Runs',
  'Tools',
  'MCP',
  'Memory',
  'Evals',
  'Models',
  'Settings',
]
const bottomPanelItems = ['Terminal', 'Logs', 'Problems', 'Event Stream']

export function AppShell({ children }: { children: ReactNode }) {
  return (
    <div className="grid min-h-screen min-w-0 grid-rows-[auto_minmax(0,1fr)_auto] bg-background text-foreground">
      <header className="border-border border-b bg-surface">
        <div className="flex h-12 items-center justify-between gap-4 px-4">
          <div className="flex items-center gap-4">
            <div className="font-semibold tracking-normal">Jyowo</div>
            <div className="text-muted-foreground text-sm">Workspace</div>
          </div>
          <div className="flex items-center gap-4 text-muted-foreground text-sm">
            <span>Model</span>
            <span>Run idle</span>
            <span>tauri2-react</span>
          </div>
        </div>
      </header>
      <div className="grid min-h-0 grid-cols-[220px_minmax(0,1fr)_320px]">
        <nav aria-label="Primary" className="min-h-0 border-border border-r bg-surface">
          <ul className="flex flex-col gap-1 p-3">
            {primaryNavigationItems.map((item) => (
              <li key={item}>
                <button
                  className="w-full rounded-md px-3 py-2 text-left text-sm text-muted-foreground hover:bg-muted hover:text-foreground"
                  type="button"
                >
                  {item}
                </button>
              </li>
            ))}
          </ul>
        </nav>
        <main className="min-w-0 overflow-auto px-6 py-6">{children}</main>
        <aside aria-label="Inspector" className="min-h-0 border-border border-l bg-surface">
          <div className="border-border border-b px-4 py-3 font-medium text-sm">Inspector</div>
          <div className="p-4 text-muted-foreground text-sm">No event selected</div>
        </aside>
      </div>
      <section aria-label="Bottom panel" className="border-border border-t bg-surface">
        <div className="flex h-10 items-center gap-4 px-4 text-muted-foreground text-sm">
          {bottomPanelItems.map((item) => (
            <button className="hover:text-foreground" key={item} type="button">
              {item}
            </button>
          ))}
        </div>
      </section>
    </div>
  )
}
