import type { Preview } from '@storybook/react-vite'
import { QueryClient } from '@tanstack/react-query'

import { AppProviders } from '@/app/providers'
import { createMockCommandClient } from '@/shared/tauri/mock-client'
import '@/shared/styles/global.css'

const preview: Preview = {
  decorators: [
    (Story) => (
      <AppProviders
        commandClient={createMockCommandClient()}
        queryClient={
          new QueryClient({
            defaultOptions: {
              queries: {
                retry: false,
              },
            },
          })
        }
      >
        <Story />
      </AppProviders>
    ),
  ],
}

export default preview
