'use client'

import { openUrl } from '@tauri-apps/plugin-opener'
import { FolderOpen, ImagePlus, LoaderCircle, Settings } from 'lucide-react'
import Image from 'next/image'
import { useState, type ComponentProps } from 'react'
import { useTranslation } from 'react-i18next'

import { AboutDialog } from '@/components/app/AboutDialog'
import { WindowControls } from '@/components/app/WindowChrome'
import { call, dispatch } from '@/lib/backend'
import { selectableLayer } from '@/lib/geometry'
import { commands, type Operation, type Scope, type Stage } from '@/lib/protocol'
import {
  pageKey,
  pagesKey,
  projectKey,
  refresh,
  useImportPages,
  usePage,
  usePages,
  useProject,
} from '@/lib/queries'
import { useKoharuStore } from '@/lib/store'
import {
  Menubar,
  MenubarContent as UiMenubarContent,
  MenubarItem as UiMenubarItem,
  MenubarMenu,
  MenubarSeparator as UiMenubarSeparator,
  MenubarShortcut as UiMenubarShortcut,
  MenubarSub,
  MenubarSubContent,
  MenubarSubTrigger,
  MenubarTrigger as UiMenubarTrigger,
} from '@koharu/ui/components/menubar'
import { cn } from '@koharu/ui/lib/utils'

export function TitleBar() {
  const { t } = useTranslation()
  const [aboutOpen, setAboutOpen] = useState(false)
  const project = useProject().data
  const pagesQuery = usePages(Boolean(project))
  const pageQuery = usePage(Boolean(project))
  const pages = project ? (pagesQuery.data ?? []) : []
  const page = project ? (pageQuery.data ?? null) : null
  const selectedPages = useKoharuStore((state) => state.selectedPages)
  const selectedLayers = useKoharuStore((state) => state.selectedLayers)
  const selectLayers = useKoharuStore((state) => state.selectLayers)
  const setSettingsOpen = useKoharuStore((state) => state.setSettingsOpen)
  const { importPages, importing } = useImportPages()

  const run = (scope: Scope, operation: Operation = { operation: 'full' }) =>
    void call(commands.process, scope, operation).catch(() => undefined)

  const closeProject = () => void call(commands.closeProject).catch(() => undefined)

  return (
    <>
      <header
        data-tauri-drag-region='deep'
        className='flex h-10 shrink-0 items-center bg-[var(--surface-titlebar)] text-[12px]'
      >
        <div className='flex h-full w-10 shrink-0 items-center justify-center rounded-br-lg'>
          <Image
            className='pointer-events-none'
            src='/icon.png'
            alt='Koharu'
            width={17}
            height={17}
            draggable={false}
            priority
          />
        </div>
        <Menubar className='h-full shrink-0 gap-0 border-0 bg-transparent p-0 shadow-none'>
          <MenubarMenu>
            <MenubarTrigger>{t('native.menu.file', { defaultValue: 'File' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarSub>
                <MenubarSubTrigger
                  disabled={!project || importing}
                  aria-busy={importing}
                  className='min-h-8 gap-1.5 px-2 py-1 text-xs'
                >
                  {importing && <LoaderCircle className='animate-spin' aria-hidden='true' />}
                  {importing
                    ? t('native.navigator.importing', { defaultValue: 'Importing pages…' })
                    : t('native.menu.importPages', { defaultValue: 'Import Pages…' })}
                </MenubarSubTrigger>
                <MenubarSubContent className='min-w-40 p-1'>
                  <MenubarItem disabled={importing} onClick={() => importPages('files')}>
                    <ImagePlus />
                    {t('native.navigator.importFiles', { defaultValue: 'Files...' })}
                  </MenubarItem>
                  <MenubarItem disabled={importing} onClick={() => importPages('folder')}>
                    <FolderOpen />
                    {t('native.navigator.importFolder', { defaultValue: 'Folder...' })}
                  </MenubarItem>
                </MenubarSubContent>
              </MenubarSub>
              <MenubarItem
                disabled={!project || pages.length === 0}
                onClick={() =>
                  void call(commands.exportPages, exportSelection(selectedPages, page?.id), 'png')
                }
              >
                {t('native.menu.exportPng', { defaultValue: 'Export PNG…' })}
              </MenubarItem>
              <MenubarItem
                disabled={!project || pages.length === 0}
                onClick={() =>
                  void call(commands.exportPages, exportSelection(selectedPages, page?.id), 'psd')
                }
              >
                {t('native.menu.exportPsd', { defaultValue: 'Export PSD…' })}
              </MenubarItem>
              <MenubarSeparator />
              <MenubarItem disabled={!project} onClick={closeProject}>
                {t('native.menu.closeProject', { defaultValue: 'Close Project' })}
              </MenubarItem>
              <MenubarSeparator />
              <MenubarItem onClick={() => setSettingsOpen(true)}>
                <Settings />
                {t('native.menu.settings', { defaultValue: 'Settings' })}
              </MenubarItem>
            </MenubarContent>
          </MenubarMenu>

          <MenubarMenu>
            <MenubarTrigger>{t('native.menu.edit', { defaultValue: 'Edit' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarItem
                disabled={!project?.can_undo}
                onClick={() =>
                  void call(commands.undo)
                    .then(() => refresh(projectKey, pagesKey, pageKey))
                    .catch(() => undefined)
                }
              >
                {t('native.menu.undo', { defaultValue: 'Undo' })}
                <MenubarShortcut>Ctrl+Z</MenubarShortcut>
              </MenubarItem>
              <MenubarItem
                disabled={!project?.can_redo}
                onClick={() =>
                  void call(commands.redo)
                    .then(() => refresh(projectKey, pagesKey, pageKey))
                    .catch(() => undefined)
                }
              >
                {t('native.menu.redo', { defaultValue: 'Redo' })}
                <MenubarShortcut>Ctrl+Shift+Z</MenubarShortcut>
              </MenubarItem>
              <MenubarSeparator />
              <MenubarItem
                disabled={!page}
                onClick={() =>
                  selectLayers(page?.layers.filter(selectableLayer).map((layer) => layer.id) ?? [])
                }
              >
                {t('native.menu.selectAllLayers', { defaultValue: 'Select All Layers' })}
                <MenubarShortcut>Ctrl+A</MenubarShortcut>
              </MenubarItem>
              <MenubarItem
                disabled={selectedLayers.length === 0}
                variant='destructive'
                onClick={() =>
                  void call(commands.deleteLayers, selectedLayers)
                    .then(() => refresh(projectKey, pagesKey, pageKey))
                    .catch(() => undefined)
                }
              >
                {t('native.menu.delete', { defaultValue: 'Delete Selected' })}
                <MenubarShortcut>Del</MenubarShortcut>
              </MenubarItem>
            </MenubarContent>
          </MenubarMenu>

          <MenubarMenu>
            <MenubarTrigger>{t('native.menu.process', { defaultValue: 'Process' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarItem
                disabled={!project || pages.length === 0}
                onClick={() => run({ scope: 'project' })}
              >
                {t('native.menu.processProject', { defaultValue: 'Process Project' })}
              </MenubarItem>
              <MenubarItem
                disabled={selectedPages.length === 0}
                onClick={() => run({ scope: 'pages', value: selectedPages })}
              >
                {t('native.menu.processPages', { defaultValue: 'Process Selected Pages' })}
              </MenubarItem>
              <MenubarItem
                disabled={selectedLayers.length === 0}
                onClick={() =>
                  run(
                    { scope: 'entities', value: selectedLayers },
                    { operation: 'stages', stages: ['ocr', 'translation'] },
                  )
                }
              >
                {t('native.menu.processLayers', { defaultValue: 'Process Selection' })}
              </MenubarItem>
              <MenubarSeparator />
              {(['detection', 'ocr', 'translation', 'inpainting'] as Stage[]).map((stage) => (
                <MenubarItem
                  key={stage}
                  disabled={!project || pages.length === 0}
                  onClick={() => run({ scope: 'project' }, { operation: 'through', stage })}
                >
                  {t('native.menu.runPhase', {
                    phase: t(`native.phase.${stage}`, { defaultValue: stage }),
                    defaultValue: 'Run {{phase}}',
                  })}
                </MenubarItem>
              ))}
            </MenubarContent>
          </MenubarMenu>

          <MenubarMenu>
            <MenubarTrigger>{t('native.menu.view', { defaultValue: 'View' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarItem disabled={!page} onClick={() => dispatch(commands.fitCanvas)}>
                {t('native.menu.fit', { defaultValue: 'Fit Window' })}
              </MenubarItem>
            </MenubarContent>
          </MenubarMenu>

          <MenubarMenu>
            <MenubarTrigger>{t('menu.help', { defaultValue: 'Help' })}</MenubarTrigger>
            <MenubarContent>
              <MenubarItem
                onClick={() => void openUrl('https://discord.gg/mHvHkxGnUY').catch(() => undefined)}
              >
                {t('menu.discord', { defaultValue: 'Discord' })}
              </MenubarItem>
              <MenubarItem
                onClick={() =>
                  void openUrl('https://github.com/mayocream/koharu').catch(() => undefined)
                }
              >
                {t('menu.github', { defaultValue: 'GitHub' })}
              </MenubarItem>
              <MenubarSeparator />
            <MenubarItem onClick={() => setAboutOpen(true)}>About</MenubarItem>
            </MenubarContent>
          </MenubarMenu>
        </Menubar>

        <div className='flex h-full min-w-16 flex-1 items-center justify-center px-3 text-[11px] text-muted-foreground select-none'>
          {project ? (
            <span className='pointer-events-none max-w-[40vw] truncate'>
              <span className='font-medium text-foreground'>{project.name}</span>
              {page && <span className='ml-2'>/ {page.label}</span>}
            </span>
          ) : (
            <span className='pointer-events-none'>Koharu</span>
          )}
        </div>

        <WindowControls />
      </header>
      <AboutDialog open={aboutOpen} onOpenChange={setAboutOpen} />
    </>
  )
}

function exportSelection(selected: string[], active?: string): string[] {
  if (selected.length) return selected
  return active ? [active] : []
}

function MenubarTrigger({ className, ...props }: ComponentProps<typeof UiMenubarTrigger>) {
  return (
    <UiMenubarTrigger
      className={cn(
        'h-6 px-1.5 text-[11px] text-muted-foreground transition-colors hover:bg-primary/10 hover:text-muted-foreground aria-expanded:bg-primary/10 aria-expanded:text-muted-foreground',
        className,
      )}
      {...props}
    />
  )
}

function MenubarContent({ className, ...props }: ComponentProps<typeof UiMenubarContent>) {
  return <UiMenubarContent className={cn('min-w-44 p-1', className)} {...props} />
}

function MenubarItem({ className, ...props }: ComponentProps<typeof UiMenubarItem>) {
  return (
    <UiMenubarItem
      className={cn(
        "min-h-8 gap-1.5 px-2 py-1 text-xs [&_svg:not([class*='size-'])]:size-3.5",
        className,
      )}
      {...props}
    />
  )
}

function MenubarShortcut({ className, ...props }: ComponentProps<typeof UiMenubarShortcut>) {
  return <UiMenubarShortcut className={cn('text-[11px]', className)} {...props} />
}

function MenubarSeparator({ className, ...props }: ComponentProps<typeof UiMenubarSeparator>) {
  return <UiMenubarSeparator className={cn('my-0.5', className)} {...props} />
}
