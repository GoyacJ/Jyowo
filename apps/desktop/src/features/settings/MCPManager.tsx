import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { Save, Server } from 'lucide-react'
import { useForm } from 'react-hook-form'
import { z } from 'zod'

import { listMcpServers, type SaveMcpServerRequest, saveMcpServer } from '@/shared/tauri/commands'
import { useCommandClient } from '@/shared/tauri/react'
import { Button } from '@/shared/ui/button'

import { MCPServerCard } from './MCPServerCard'

const mcpServerQueryKeys = {
  all: ['mcp-servers'] as const,
  list: () => [...mcpServerQueryKeys.all, 'list'] as const,
}

const mcpServerFormSchema = z
  .object({
    args: z.string(),
    command: z.string().trim().min(1, 'Command is required.'),
    displayName: z.string().trim().min(1, 'Server name is required.'),
    id: z
      .string()
      .trim()
      .min(1, 'Server id is required.')
      .regex(
        /^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$/,
        'Use letters, numbers, dot, dash, or underscore.',
      ),
    scope: z.enum(['global', 'session', 'agent']),
  })
  .strict()

type MCPServerFormValues = z.infer<typeof mcpServerFormSchema>

export function MCPManager() {
  const commandClient = useCommandClient()
  const queryClient = useQueryClient()
  const {
    formState: { errors, isSubmitting },
    handleSubmit,
    register,
    reset,
    setError,
  } = useForm<MCPServerFormValues>({
    defaultValues: {
      args: '',
      command: '',
      displayName: '',
      id: '',
      scope: 'global',
    },
  })
  const serversQuery = useQuery({
    queryKey: mcpServerQueryKeys.list(),
    queryFn: () => listMcpServers(commandClient),
  })
  const saveMutation = useMutation({
    mutationFn: (request: SaveMcpServerRequest) => saveMcpServer(request, commandClient),
    onSuccess: async () => {
      reset()
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const deleteMutation = useMutation({
    mutationFn: (id: string) => commandClient.deleteMcpServer(id),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: mcpServerQueryKeys.all })
    },
  })
  const servers = serversQuery.data?.servers ?? []

  async function submit(values: MCPServerFormValues) {
    const parsed = mcpServerFormSchema.safeParse(values)

    if (!parsed.success) {
      const handledFields = new Set<string>()
      for (const issue of parsed.error.issues) {
        const field = issue.path[0]
        if (
          field === 'args' ||
          field === 'command' ||
          field === 'displayName' ||
          field === 'id' ||
          field === 'scope'
        ) {
          if (handledFields.has(field)) {
            continue
          }
          setError(field, { message: issue.message, type: 'manual' })
          handledFields.add(field)
        }
      }
      return
    }

    try {
      await saveMutation.mutateAsync({
        displayName: parsed.data.displayName,
        id: parsed.data.id,
        scope: parsed.data.scope,
        transport: {
          args: splitArgs(parsed.data.args),
          command: parsed.data.command,
          kind: 'stdio',
        },
      })
    } catch {
      // The rendered message is intentionally sanitized and does not use backend error text.
    }
  }

  async function deleteServer(id: string) {
    await deleteMutation.mutateAsync(id)
  }

  return (
    <section className="space-y-5 rounded-md border border-border bg-surface p-5">
      <div className="flex items-start gap-3">
        <div className="rounded-md border border-border bg-background p-2 text-muted-foreground">
          <Server className="size-4" />
        </div>
        <div>
          <h2 className="font-semibold text-base">MCP servers</h2>
          <p className="mt-1 text-muted-foreground text-sm">
            Manage runtime tool servers for this workspace.
          </p>
        </div>
      </div>

      <form className="grid gap-4 md:grid-cols-2" onSubmit={handleSubmit(submit)}>
        <label className="space-y-2 text-sm">
          <span className="font-medium">Server name</span>
          <input
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            disabled={isSubmitting}
            placeholder="Workspace GitHub"
            {...register('displayName')}
          />
          {errors.displayName ? (
            <span className="block text-destructive text-xs">{errors.displayName.message}</span>
          ) : null}
        </label>

        <label className="space-y-2 text-sm">
          <span className="font-medium">Server id</span>
          <input
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            disabled={isSubmitting}
            placeholder="github"
            {...register('id')}
          />
          {errors.id ? (
            <span className="block text-destructive text-xs">{errors.id.message}</span>
          ) : null}
        </label>

        <label className="space-y-2 text-sm">
          <span className="font-medium">Command</span>
          <input
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            disabled={isSubmitting}
            placeholder="node"
            {...register('command')}
          />
          {errors.command ? (
            <span className="block text-destructive text-xs">{errors.command.message}</span>
          ) : null}
        </label>

        <label className="space-y-2 text-sm">
          <span className="font-medium">Arguments</span>
          <input
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            disabled={isSubmitting}
            placeholder="mcp-server"
            {...register('args')}
          />
        </label>

        <label className="space-y-2 text-sm">
          <span className="font-medium">Scope</span>
          <select
            className="h-10 w-full rounded-md border border-border bg-background px-3 text-sm outline-none focus-visible:ring-2 focus-visible:ring-ring"
            disabled={isSubmitting}
            {...register('scope')}
          >
            <option value="global">Global</option>
            <option value="session">Session</option>
            <option value="agent">Agent</option>
          </select>
        </label>

        <div className="flex items-end justify-end">
          <Button disabled={isSubmitting} type="submit">
            <Save className="size-4" />
            {isSubmitting ? 'Saving MCP server' : 'Save MCP server'}
          </Button>
        </div>
      </form>

      {saveMutation.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          MCP server could not be saved.
        </div>
      ) : null}

      {serversQuery.isError ? (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-2 text-destructive text-sm">
          MCP servers could not be loaded.
        </div>
      ) : null}

      {serversQuery.isLoading ? (
        <div className="text-muted-foreground text-sm">Loading MCP servers.</div>
      ) : null}

      {!serversQuery.isLoading && servers.length === 0 ? (
        <div className="rounded-md border border-dashed border-border bg-background px-4 py-6 text-center text-muted-foreground text-sm">
          No MCP servers configured.
        </div>
      ) : null}

      {servers.length > 0 ? (
        <div className="space-y-3">
          {servers.map((server) => (
            <MCPServerCard
              key={server.id}
              onDelete={deleteMutation.isPending ? () => undefined : deleteServer}
              server={server}
            />
          ))}
        </div>
      ) : null}
    </section>
  )
}

function splitArgs(input: string): string[] {
  return input
    .split(/\s+/)
    .map((part) => part.trim())
    .filter(Boolean)
}
