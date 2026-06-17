# Jyowo Tauri IPC

## Boundary

The frontend is not a trusted boundary.

- MUST expose IPC through `shared/tauri`.
- MUST use `CommandClient` as the only frontend IPC abstraction.
- MUST validate every command payload with Zod.
- MUST normalize invalid payload errors.
- MUST NOT call `invoke` inside React components.
- MUST NOT add shell, filesystem, process, network, or sidecar permissions in this phase.

## Current Commands

```ts
getAppInfo(): {
  name: 'Jyowo'
  version: string
  shell: 'tauri2-react'
  harness: {
    sdkCrate: 'jyowo_harness_sdk'
    mode: 'in-process'
  }
}
```

```ts
getHarnessHealthcheck(): {
  status: 'available'
  sdkCrate: 'jyowo_harness_sdk'
}
```

## Runtime Clients

- Production MUST use the Tauri invoke client.
- Unit tests MUST use mock clients.
- Storybook MUST use mock clients.
- Playwright web smoke tests MUST use mock clients.

## Capabilities

`apps/desktop/src-tauri/capabilities/default.json` MUST remain limited to `core:default` until a separate permission design is accepted.
