import { open } from '@tauri-apps/plugin-dialog'

import type { AttachmentInputModality } from './commands'

const defaultAttachmentModalities: AttachmentInputModality[] = ['image', 'video', 'file']

const attachmentFilters: Record<AttachmentInputModality, { name: string; extensions: string[] }> = {
  image: {
    name: 'Images',
    extensions: ['png', 'jpg', 'jpeg', 'webp', 'gif', 'heic', 'heif'],
  },
  video: {
    name: 'Videos',
    extensions: ['mp4', 'mov', 'webm', 'mkv', 'avi'],
  },
  file: {
    name: 'Documents',
    extensions: [
      'txt',
      'md',
      'markdown',
      'pdf',
      'csv',
      'tsv',
      'json',
      'yaml',
      'yml',
      'toml',
      'xml',
      'doc',
      'docx',
      'xls',
      'xlsx',
      'ppt',
      'pptx',
      'rtf',
      'zip',
    ],
  },
}

export async function pickAttachmentPath(
  modalities: AttachmentInputModality[] = defaultAttachmentModalities,
): Promise<string | null> {
  const filters = acceptedAttachmentFilters(modalities)
  if (filters.length === 0) {
    return null
  }

  const selected = await open({
    directory: false,
    filters,
    multiple: false,
  })

  return typeof selected === 'string' ? selected : null
}

function acceptedAttachmentFilters(modalities: AttachmentInputModality[]) {
  const uniqueModalities = defaultAttachmentModalities.filter((modality) =>
    modalities.includes(modality),
  )
  return uniqueModalities.map((modality) => attachmentFilters[modality])
}

export async function pickProjectDirectory(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
  })

  return typeof selected === 'string' ? selected : null
}

export async function pickSkillPackagePath(): Promise<string | null> {
  const selected = await open({
    directory: true,
    multiple: false,
  })

  return typeof selected === 'string' ? selected : null
}
