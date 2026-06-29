import type { Preview } from '@storybook/react-vite'
import { QueryClient } from '@tanstack/react-query'

import { AppProviders } from '@/app/providers'
import '@/shared/styles/global.css'
import { createTestCommandClient } from '@/testing/command-client'

const preview: Preview = {
  decorators: [
    (Story) => (
      <AppProviders
        commandClient={createTestCommandClient()}
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
