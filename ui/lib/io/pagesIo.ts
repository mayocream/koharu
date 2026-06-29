'use client'

import { getGetSceneJsonQueryKey, importTranslations as importTranslationsApi } from '@/lib/api/default/default'
import type { ImportTranslationsResponse, SceneSnapshot } from '@/lib/api/schemas'
import { openImageFiles, openImageFolder, openJsonFile, openKhrFile } from '@/lib/io/openFiles'
import { saveBlob } from '@/lib/io/saveBlob'
import { exportProject, uploadKhrArchive, uploadPages, uploadPagesByPaths } from '@/lib/io/scene'
import { queryClient } from '@/lib/queryClient'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

/**
 * Platform-neutral image import. `openImageFiles` / `openImageFolder` return
 * `File[]` on both Tauri and the web; the upload + scene invalidation lives
 * in `lib/io/scene.ts` on top of the orval-generated `createPages` mutation.
 */
export async function importPages(
  mode: 'replace' | 'append',
  source: 'files' | 'folder',
): Promise<void> {
  const picked = source === 'folder' ? await openImageFolder() : await openImageFiles()
  const replace = mode === 'replace'
  if (picked.kind === 'paths') {
    if (picked.paths.length === 0) return
    await uploadPagesByPaths(picked.paths, replace)
    return
  }
  if (picked.files.length === 0) return
  await uploadPages(picked.files, replace)
}

/**
 * Import a `.khr` archive. Works on both desktop and web: the archive file
 * is picked via the cross-platform `openKhrFile`, and the destination is
 * allocated server-side so the client never needs to touch the filesystem.
 */
export async function importKhrFile(): Promise<void> {
  const file = await openKhrFile()
  if (!file) return
  await uploadKhrArchive(file)
}

// ---------------------------------------------------------------------------
// Export (server returns bytes; saveBlob dispatches Tauri-dialog / web-FS)
// ---------------------------------------------------------------------------

const exportExtension: Record<
  'khr' | 'psd' | 'rendered' | 'inpainted' | 'source_texts' | 'translations',
  string
> = {
  khr: 'khr',
  psd: 'zip',
  rendered: 'zip',
  inpainted: 'zip',
  source_texts: 'json',
  translations: 'json',
}

/** Sanitise an arbitrary project name for use as a filename stem. */
function sanitiseBaseName(name: string | undefined | null): string {
  const cleaned = (name ?? '')
    .trim()
    .replace(/[\\/:*?"<>|]+/g, '_')
    .replace(/\s+/g, ' ')
  return cleaned.length > 0 ? cleaned : 'koharu-export'
}

/** Read the current project name from React Query's cached scene snapshot. */
function currentProjectName(): string | undefined {
  const snap = queryClient.getQueryData<SceneSnapshot>(getGetSceneJsonQueryKey())
  return snap?.scene.project?.name ?? undefined
}

export async function exportCurrentProjectAs(
  format: 'khr' | 'psd' | 'rendered' | 'inpainted' | 'source_texts' | 'translations',
  pages?: string[],
): Promise<void> {
  try {
    const defaultFont = usePreferencesStore.getState().defaultFont
    const { blob, filename } = await exportProject({ format, pages, defaultFont })
    const base = sanitiseBaseName(currentProjectName())
    // Prefer the server's Content-Disposition filename (matches the actual
    // bytes — a raw PNG/PSD for single-file responses, a zip for multi).
    // Fall back to our guess only if the header is missing/unparseable.
    const defaultName = filename ?? `${base}.${exportExtension[format]}`
    await saveBlob(blob, defaultName)
  } catch (err) {
    console.error('Export failed:', err)
    throw err
  }
}

/**
 * Import a translations document. The user picks a JSON file; the server
 * applies the `texts[]` payload to each text node's translation slot in
 * reading order, matching pages by their 1-indexed `page` number.
 *
 * Returns a summary describing how many pages were applied and which were
 * skipped. The caller is responsible for surfacing the summary to the user
 * (the menu bar uses `alert()` as a placeholder).
 */
export async function importTranslations(): Promise<ImportTranslationsResponse | null> {
  const file = await openJsonFile()
  if (!file) return null
  const payload = await file.text()
  const result = await importTranslationsApi({ payload })
  await queryClient.invalidateQueries({ queryKey: getGetSceneJsonQueryKey() })
  return result
}
