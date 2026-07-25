'use client'

import { findImageBlob } from '@/hooks/useCurrentPage'
import { getGetSceneJsonQueryKey, getSceneJson } from '@/lib/api/default/default'
import type { Scene, SceneSnapshot } from '@/lib/api/schemas'
import { isTauri } from '@/lib/backend'
import { openImageFiles, openImageFolder, openKhrFile } from '@/lib/io/openFiles'
import { pickSaveDirectory, saveBlob, writeFileInDir } from '@/lib/io/saveBlob'
import { exportProject, uploadKhrArchive, uploadPages, uploadPagesByPaths } from '@/lib/io/scene'
import { queryClient } from '@/lib/queryClient'
import { useEditorUiStore } from '@/lib/stores/editorUiStore'
import { type ExportRole, useExportStore } from '@/lib/stores/exportStore'
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
 * Surface a translated message in the activity bubble. `lib/i18n` is imported
 * lazily so the io layer doesn't pull every locale bundle into its graph for
 * the (common) case where nothing goes wrong.
 */
async function showExportError(key: string, opts?: Record<string, unknown>): Promise<void> {
  const { default: i18n } = await import('@/lib/i18n')
  useEditorUiStore.getState().showError(i18n.t(key, opts ?? {}))
}

/** Read the scene from React Query's cache, fetching it only if absent. */
async function currentScene(): Promise<Scene> {
  const cached = queryClient.getQueryData<SceneSnapshot>(getGetSceneJsonQueryKey())
  if (cached) return cached.scene
  return (await getSceneJson()).scene
}

/**
 * Streaming multi-page image export (desktop only).
 *
 * Asks for the destination folder **first**, then exports one page per
 * request and writes it straight to disk. Peak memory is a single page, no
 * matter how many pages the project has — the old path buffered every PNG
 * server-side, zipped that into a second copy, shipped it as one blob and
 * only then opened the folder dialog, which OOM'd on large projects.
 */
export async function exportImagesToFolder(role: ExportRole, pages?: string[]): Promise<void> {
  const scene = await currentScene()

  // `pages` order (or scene insertion order) is the export order, and the
  // `page-NNN-` prefix counts positions in *that* list — including pages that
  // get skipped below for lacking the layer. This mirrors the server's naming
  // so a folder export is byte-for-byte what the zip export contained.
  const resolved: (readonly [string, Scene['pages'][string]])[] = pages
    ? pages.map((id) => [id, scene.pages[id]] as const)
    : Object.entries(scene.pages)
  for (const [id, page] of resolved) {
    if (!page) throw new Error(`page ${id} not found`)
  }
  const work = resolved
    .map(([id, page], index) => ({ id, page, index }))
    .filter(({ page }) => findImageBlob(page, role) !== null)

  // Same condition the server rejects with, caught client-side so the user
  // gets told instead of being handed a folder dialog for an empty export.
  if (work.length === 0) {
    await showExportError('operations.exportNoPages')
    return
  }

  const dir = await pickSaveDirectory()
  if (!dir) return

  const store = useExportStore
  store.getState().start(role, work.length)
  // Abort the in-flight request too, so Cancel doesn't wait out a page.
  const controller = new AbortController()
  const unsubscribe = store.subscribe((s) => {
    if (s.cancelRequested) controller.abort()
  })
  const failures: string[] = []

  try {
    for (const { id, index } of work) {
      if (store.getState().cancelRequested) break
      const name = `page-${String(index + 1).padStart(3, '0')}-${id}.png`
      try {
        const { blob } = await exportProject({ format: role, pages: [id] }, controller.signal)
        await writeFileInDir(dir, name, new Uint8Array(await blob.arrayBuffer()))
      } catch (err) {
        if (controller.signal.aborted) break
        failures.push(`${name}: ${String(err)}`)
      }
      // Nothing is retained between iterations — the blob is garbage now.
      store.getState().advance(name)
    }
  } finally {
    unsubscribe()
    store.getState().finish()
  }

  if (failures.length > 0) {
    console.error('Export failures:', failures)
    // Deliberately not named `count` — i18next would treat it as a plural
    // selector and look for `exportFailures_one` / `_other`.
    await showExportError('operations.exportFailures', {
      failed: failures.length,
      total: work.length,
    })
  }
}

export async function exportCurrentProjectAs(
  format: 'khr' | 'psd' | 'rendered' | 'inpainted',
  pages?: string[],
): Promise<void> {
  try {
    // Multi-page image exports stream page-by-page into a folder on desktop.
    // Everything else (khr, psd, single page, browser) takes the blob path.
    if (isTauri() && (format === 'rendered' || format === 'inpainted') && pages?.length !== 1) {
      await exportImagesToFolder(format, pages)
      return
    }
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
