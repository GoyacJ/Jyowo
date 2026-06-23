import { open } from '@tauri-apps/plugin-dialog'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { pickAttachmentPath } from './file-dialog'

const openMock = vi.hoisted(() => vi.fn())

vi.mock('@tauri-apps/plugin-dialog', () => ({
  open: openMock,
}))

describe('file-dialog', () => {
  beforeEach(() => {
    openMock.mockReset()
  })

  it('filters attachment selection by accepted model modalities', async () => {
    openMock.mockResolvedValue('/tmp/photo.png')

    await expect(pickAttachmentPath(['image', 'video'])).resolves.toBe('/tmp/photo.png')

    expect(open).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      filters: [
        {
          name: 'Images',
          extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'heic', 'heif'],
        },
        {
          name: 'Videos',
          extensions: ['mp4', 'mov', 'webm', 'mkv', 'avi'],
        },
      ],
    })
  })

  it('does not add a broad document filter unless file input is accepted', async () => {
    openMock.mockResolvedValue('/tmp/movie.mp4')

    await pickAttachmentPath(['video'])

    expect(open).toHaveBeenCalledWith({
      directory: false,
      multiple: false,
      filters: [
        {
          name: 'Videos',
          extensions: ['mp4', 'mov', 'webm', 'mkv', 'avi'],
        },
      ],
    })
  })
})
