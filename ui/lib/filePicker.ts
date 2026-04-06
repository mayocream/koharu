'use client'

import { fileOpen, directoryOpen } from 'browser-fs-access'

const IMAGE_EXTENSIONS = ['.png', '.jpg', '.jpeg', '.webp']
const ARCHIVE_EXTENSIONS = ['.zip', '.cbz']

export const pickImageFiles = async (): Promise<File[] | null> => {
  try {
    const files = await fileOpen({
      mimeTypes: ['image/*', 'application/zip', 'application/x-cbz'],
      extensions: [...IMAGE_EXTENSIONS, ...ARCHIVE_EXTENSIONS],
      multiple: true,
      description: 'Select images or archives',
    })
    const result = Array.isArray(files) ? files : [files]
    return result.length > 0 ? result : null
  } catch {
    return null // user cancelled
  }
}

export const pickImageFolderFiles = async (): Promise<File[] | null> => {
  try {
    const files = await directoryOpen({ recursive: true })
    const images = files.filter((f) =>
      IMAGE_EXTENSIONS.some((ext) => f.name.toLowerCase().endsWith(ext)),
    )
    return images.length > 0 ? images : null
  } catch {
    return null // user cancelled
  }
}
