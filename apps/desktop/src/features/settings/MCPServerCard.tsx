import { RotateCw, Settings, Trash2 } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import type { McpServerSummary } from '@/shared/tauri/commands'
import { Badge, type BadgeProps } from '@/shared/ui/badge'
import { Button } from '@/shared/ui/button'
import { Switch } from '@/shared/ui/switch'
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from '@/shared/ui/tooltip'

interface MCPServerCardProps {
  onConfigure: (server: McpServerSummary) => void
  onDelete: (id: string) => void
  onRestart: (id: string) => void
  onToggle: (id: string, enabled: boolean) => void
  server: McpServerSummary
}

export function MCPServerCard({
  onConfigure,
  onDelete,
  onRestart,
  onToggle,
  server,
}: MCPServerCardProps) {
  const { t } = useTranslation('settings')

  return (
    <article
      aria-label={server.displayName}
      className="rounded-md border border-border bg-background px-3 py-3"
    >
      <div className="grid gap-3 md:grid-cols-[minmax(0,1fr)_auto] md:items-start">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <h4 className="truncate font-medium text-sm">{server.displayName}</h4>
          </div>
          <div className="mt-2 flex flex-wrap gap-2">
            <Badge variant={statusVariant(server.status)}>{server.status}</Badge>
            <Badge variant="outline">{server.transport}</Badge>
            <Badge variant="outline">{server.origin}</Badge>
            <Badge variant="outline">{server.scope}</Badge>
            <Badge variant="outline">
              {t('mcp.toolCount', { count: server.exposedToolCount })}
            </Badge>
          </div>
          {server.lastDiagnostic ? (
            <div className="mt-2 flex flex-wrap items-center gap-2 text-muted-foreground text-xs">
              <span>{t('mcp.lastDiagnostic')}</span>
              <span>{server.lastDiagnostic}</span>
            </div>
          ) : null}
          {server.lastError ? (
            <div className="mt-2 rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-xs">
              {server.lastError}
            </div>
          ) : null}
        </div>

        <TooltipProvider>
          <div className="flex items-center gap-1 md:justify-end">
            {server.manageable ? (
              <>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Switch
                      aria-label={t(server.enabled ? 'mcp.disableServer' : 'mcp.enableServer', {
                        name: server.displayName,
                      })}
                      checked={server.enabled}
                      onCheckedChange={(enabled) => onToggle(server.id, enabled)}
                    />
                  </TooltipTrigger>
                  <TooltipContent>
                    {t(server.enabled ? 'mcp.disable' : 'mcp.enable')}
                  </TooltipContent>
                </Tooltip>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      aria-label={t('mcp.configureServer', { name: server.displayName })}
                      onClick={() => onConfigure(server)}
                      size="icon"
                      type="button"
                      variant="ghost"
                    >
                      <Settings className="size-4" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>{t('mcp.configure')}</TooltipContent>
                </Tooltip>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      aria-label={t('mcp.restartServer', { name: server.displayName })}
                      onClick={() => onRestart(server.id)}
                      size="icon"
                      type="button"
                      variant="ghost"
                    >
                      <RotateCw className="size-4" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>{t('mcp.restart')}</TooltipContent>
                </Tooltip>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      aria-label={t('mcp.deleteServer', { name: server.displayName })}
                      onClick={() => onDelete(server.id)}
                      size="icon"
                      type="button"
                      variant="ghost"
                    >
                      <Trash2 className="size-4" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>{t('mcp.delete')}</TooltipContent>
                </Tooltip>
              </>
            ) : (
              <Badge variant="outline">{t('mcp.readOnly')}</Badge>
            )}
          </div>
        </TooltipProvider>
      </div>
    </article>
  )
}

function statusVariant(status: McpServerSummary['status']): BadgeProps['variant'] {
  if (status === 'ready') {
    return 'success'
  }
  if (status === 'failed') {
    return 'destructive'
  }
  if (status === 'disabled' || status === 'configured') {
    return 'outline'
  }
  return 'secondary'
}
