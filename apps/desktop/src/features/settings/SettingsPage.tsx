import { MCPManager } from './MCPManager'
import { ProviderSettingsForm } from './ProviderSettingsForm'

export function SettingsPage() {
  return (
    <div className="mx-auto flex w-full max-w-5xl flex-col gap-5">
      <header>
        <h1 className="font-semibold text-2xl">Settings</h1>
        <p className="mt-1 text-muted-foreground text-sm">
          Configure local providers and workspace tool servers.
        </p>
      </header>

      <ProviderSettingsForm />
      <MCPManager />
    </div>
  )
}
