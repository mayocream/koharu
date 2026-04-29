'use client'

import { getExportCurrentProjectUrl, getGetSceneJsonQueryKey } from '@/lib/api/default/default'
import type { ExportProjectRequest, SceneSnapshot } from '@/lib/api/schemas'
import { ApiError } from '@/lib/api/fetch'
import { openImageFiles, openImageFolder, openKhrFile } from '@/lib/io/openFiles'
import { filenameFromContentDisposition, saveBlob, saveBlobViaStream } from '@/lib/io/saveBlob'
import { exportProject, uploadKhrArchive, uploadPages, uploadPagesByPaths } from '@/lib/io/scene'
import type { StreamingUnzipProgress } from '@/lib/io/streamingUnzip'
import { queryClient } from '@/lib/queryClient'

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
// Export
// ---------------------------------------------------------------------------

const exportExtension: Record<'khr' | 'psd' | 'rendered' | 'inpainted', string> = {
  khr: 'khr',
  psd: 'zip',
  rendered: 'zip',
  inpainted: 'zip',
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

/**
 * Export the project (or a subset of pages).
 *
 * - **ZIP formats** (`rendered`, `inpainted`, `psd`) with multiple pages:
 *   uses the streaming "先授權、後串流" path — folder picker fires first,
 *   then the response is streamed and unzipped directly to disk. No blob
 *   ever accumulates in memory.
 *
 * - **Single-file formats** (`khr`) or explicit single-page exports:
 *   uses the classic blob path (small file, dialog fires after download).
 *
 * @param onProgress  Optional callback for download + unzip progress.
 *                    Only invoked on the streaming ZIP path.
 */
export async function exportCurrentProjectAs(
  format: 'khr' | 'psd' | 'rendered' | 'inpainted',
  pages?: string[],
  onProgress?: (p: StreamingUnzipProgress) => void,
): Promise<void> {
  const isZipFormat = format !== 'khr'
  const isMultiPage = !pages || pages.length !== 1

  if (isZipFormat && isMultiPage) {
    // "先授權、後串流" path: pick folder immediately, then stream
    const req: ExportProjectRequest = { format, pages }
    const base = sanitiseBaseName(currentProjectName())
    const defaultName = `${base}.${exportExtension[format]}`
    try {
      await saveBlobViaStream(
        getExportCurrentProjectUrl(),
        {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify(req),
        },
        defaultName,
        onProgress,
      )
    } catch (err) {
      console.error('Export failed:', err)
      throw err
    }
    return
  }

  // Classic blob path (single file or KHR archive)
  try {
    const { blob, filename } = await exportProject({ format, pages })
    const base = sanitiseBaseName(currentProjectName())
    const defaultName = filename ?? `${base}.${exportExtension[format]}`
    await saveBlob(blob, defaultName)
  } catch (err) {
    console.error('Export failed:', err)
    throw err
  }
}
