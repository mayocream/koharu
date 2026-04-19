'use client'

import {
  AlertCircleIcon,
  ArrowRightIcon,
  ClockIcon,
  FileArchiveIcon,
  PlusIcon,
  XIcon,
} from 'lucide-react'
import Image from 'next/image'
import { useCallback, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { Card } from '@/components/ui/card'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { useListProjects } from '@/lib/api/default/default'
import type { ProjectSummary } from '@/lib/api/schemas'
import { importKhrFile } from '@/lib/io/pagesIo'
import { createAndOpenProject, switchProject } from '@/lib/io/scene'
import { cn } from '@/lib/utils'

type Busy = false | 'new' | 'open' | 'import'

/**
 * Project-management / welcome screen. Rendered when no project is open.
 * Server manages all project paths under `{data.path}/projects/` — clients
 * only pass `id`. Same UX in Tauri and headless browser deployments.
 */
export function WelcomeScreen() {
  const { t } = useTranslation()
  const { data: projectsData, refetch: refetchProjects } = useListProjects()
  const projects = useMemo(() => {
    const all = projectsData?.projects ?? []
    return [...all].sort((a, b) => (b.updatedAtMs ?? 0) - (a.updatedAtMs ?? 0))
  }, [projectsData])

  const [busy, setBusy] = useState<Busy>(false)
  const [error, setError] = useState<string | null>(null)
  const [newDialogOpen, setNewDialogOpen] = useState(false)

  const openById = useCallback(async (id: string) => {
    setError(null)
    setBusy('open')
    try {
      await switchProject({ id })
    } catch (e) {
      setError(`Open failed: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }, [])

  const onCreate = useCallback(async (name: string) => {
    setError(null)
    setBusy('new')
    try {
      await createAndOpenProject({ name })
    } catch (e) {
      setError(`New failed: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
      setNewDialogOpen(false)
    }
  }, [])

  const importKhr = useCallback(async () => {
    setError(null)
    setBusy('import')
    try {
      await importKhrFile()
      await refetchProjects()
    } catch (e) {
      setError(`Import failed: ${e instanceof Error ? e.message : String(e)}`)
    } finally {
      setBusy(false)
    }
  }, [refetchProjects])

  return (
    <div className='relative flex min-h-0 flex-1 items-start justify-center overflow-hidden bg-background'>
      <div
        aria-hidden
        className='pointer-events-none absolute -top-40 left-1/2 h-80 w-[720px] -translate-x-1/2 rounded-full bg-primary/10 blur-3xl'
      />

      <div className='relative z-10 mx-auto flex w-full max-w-md flex-col gap-8 px-6 pt-24 pb-10'>
        <header className='flex flex-col items-center gap-2 text-center'>
          <Image src='/icon.png' alt='Koharu' width={56} height={56} priority />
          <div className='mt-1 flex flex-col gap-0.5'>
            <h1 className='text-2xl font-semibold tracking-tight text-foreground'>
              {t('welcome.title', { defaultValue: 'Koharu' })}
            </h1>
            <p className='text-xs text-muted-foreground'>
              {t('welcome.subtitle', { defaultValue: 'Manga translation workspace' })}
            </p>
          </div>
        </header>

        {error && (
          <div className='flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive/5 px-3 py-2 text-xs text-destructive'>
            <AlertCircleIcon className='mt-0.5 h-3.5 w-3.5 shrink-0' />
            <div className='flex-1'>{error}</div>
            <button
              type='button'
              onClick={() => setError(null)}
              className='cursor-pointer text-destructive/70 hover:text-destructive'
              aria-label='dismiss'
            >
              <XIcon className='h-3.5 w-3.5' />
            </button>
          </div>
        )}

        <div className='mt-4 flex flex-col gap-2.5'>
          <PrimaryAction
            onClick={() => setNewDialogOpen(true)}
            disabled={!!busy}
            loading={busy === 'new'}
            title={t('welcome.new', { defaultValue: 'New project' })}
            description={t('welcome.newDescription', {
              defaultValue: 'Start blank, import pages later',
            })}
          />
          <SecondaryAction
            onClick={importKhr}
            disabled={!!busy}
            loading={busy === 'import'}
            icon={<FileArchiveIcon className='h-4 w-4' />}
            label={t('welcome.importKhr', { defaultValue: 'Import .khr archive' })}
          />
        </div>

        <section className='flex flex-col gap-2'>
          <div className='flex items-baseline justify-between px-0.5'>
            <h2 className='text-[10px] font-semibold tracking-[0.14em] text-muted-foreground uppercase'>
              {t('welcome.projects', { defaultValue: 'Recent' })}
            </h2>
            {projects.length > 0 && (
              <span className='text-[10px] text-muted-foreground tabular-nums'>
                {projects.length}
              </span>
            )}
          </div>
          {projects.length > 0 ? (
            <ScrollArea className='h-48 rounded-lg border border-border/60 bg-card/30'>
              <ul className='flex flex-col divide-y divide-border/40'>
                {projects.map((p) => (
                  <ProjectRow key={p.id} project={p} onOpen={openById} disabled={busy === 'open'} />
                ))}
              </ul>
            </ScrollArea>
          ) : (
            <RecentSkeleton />
          )}
        </section>
      </div>

      <NewProjectDialog
        open={newDialogOpen}
        onOpenChange={setNewDialogOpen}
        onSubmit={onCreate}
        busy={busy === 'new'}
      />
    </div>
  )
}

// ---------------------------------------------------------------------------

function PrimaryAction({
  onClick,
  disabled,
  loading,
  title,
  description,
}: {
  onClick: () => void
  disabled?: boolean
  loading?: boolean
  title: string
  description: string
}) {
  return (
    <button
      type='button'
      onClick={onClick}
      disabled={disabled}
      className={cn(
        'group relative cursor-pointer overflow-hidden rounded-xl text-left outline-none',
        'focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
        'disabled:cursor-not-allowed disabled:opacity-60',
      )}
    >
      <Card
        className={cn(
          'relative flex-row items-center gap-3 overflow-hidden rounded-xl border-primary/30 px-4 py-3',
          'bg-gradient-to-br from-primary/10 via-primary/5 to-transparent',
          loading && 'border-primary/70',
        )}
      >
        <div className='flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-primary text-primary-foreground shadow-sm shadow-primary/30'>
          <PlusIcon className='h-4 w-4' />
        </div>
        <div className='flex min-w-0 flex-1 flex-col gap-0.5'>
          <div className='text-base leading-tight font-semibold tracking-tight text-foreground'>
            {title}
          </div>
          <div className='text-xs leading-snug text-muted-foreground'>{description}</div>
        </div>
        <ArrowRightIcon className='h-4 w-4 shrink-0 text-muted-foreground' />
      </Card>
    </button>
  )
}

function SecondaryAction({
  onClick,
  disabled,
  loading,
  icon,
  label,
}: {
  onClick: () => void
  disabled?: boolean
  loading?: boolean
  icon: React.ReactNode
  label: string
}) {
  return (
    <button
      type='button'
      onClick={onClick}
      disabled={disabled}
      className={cn(
        'flex cursor-pointer items-center justify-center gap-2 rounded-lg border border-transparent px-3 py-2.5 text-sm text-muted-foreground outline-none',
        'focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background',
        'disabled:cursor-not-allowed disabled:opacity-60',
        loading && 'border-primary/40 text-foreground',
      )}
    >
      <span className='text-muted-foreground'>{icon}</span>
      <span className='font-medium'>{label}</span>
    </button>
  )
}

function RecentSkeleton() {
  const { t } = useTranslation()
  const widths = ['w-32', 'w-40', 'w-28']
  return (
    <div className='relative h-48 overflow-hidden rounded-lg border border-dashed border-border/60 bg-card/20'>
      <ul aria-hidden className='flex flex-col divide-y divide-border/30'>
        {widths.map((w, i) => (
          <li key={i} className='flex items-center gap-3 px-3 py-2'>
            <div className='h-9 w-9 shrink-0 rounded-md bg-muted/60' />
            <div className='flex min-w-0 flex-1 flex-col gap-1.5'>
              <div className={cn('h-3 rounded bg-muted/70', w)} />
              <div className='h-2 w-20 rounded bg-muted/40' />
            </div>
            <div className='h-2 w-10 rounded bg-muted/40' />
          </li>
        ))}
      </ul>
      <div className='absolute inset-0 flex items-center justify-center bg-gradient-to-t from-background/95 via-background/60 to-transparent'>
        <p className='text-center text-[11px] text-muted-foreground'>
          {t('welcome.emptyHint', {
            defaultValue: 'Your recent projects will appear here',
          })}
        </p>
      </div>
    </div>
  )
}

function ProjectRow({
  project,
  onOpen,
  disabled,
}: {
  project: ProjectSummary
  onOpen: (id: string) => void
  disabled?: boolean
}) {
  const when = project.updatedAtMs && project.updatedAtMs > 0 ? new Date(project.updatedAtMs) : null
  return (
    <li>
      <button
        type='button'
        onClick={() => onOpen(project.id)}
        disabled={disabled}
        className='flex w-full cursor-pointer items-center gap-3 px-3 py-2 text-left outline-none focus-visible:bg-accent/60 disabled:cursor-not-allowed disabled:opacity-60'
      >
        <div className='flex min-w-0 flex-1 flex-col'>
          <div className='truncate text-sm font-medium text-foreground'>{project.name}</div>
          <div className='truncate text-[11px] text-muted-foreground'>{project.id}</div>
        </div>
        {when && (
          <div className='flex shrink-0 items-center gap-1 text-[11px] text-muted-foreground'>
            <ClockIcon className='h-3 w-3' />
            {formatRelative(when)}
          </div>
        )}
      </button>
    </li>
  )
}

function formatRelative(d: Date): string {
  const diff = Date.now() - d.getTime()
  const m = 60_000
  const h = 3_600_000
  const day = 86_400_000
  if (diff < m) return 'just now'
  if (diff < h) return `${Math.floor(diff / m)}m ago`
  if (diff < day) return `${Math.floor(diff / h)}h ago`
  if (diff < day * 30) return `${Math.floor(diff / day)}d ago`
  return d.toLocaleDateString()
}

// ---------------------------------------------------------------------------

function NewProjectDialog({
  open,
  onOpenChange,
  onSubmit,
  busy,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  onSubmit: (name: string) => void
  busy: boolean
}) {
  const { t } = useTranslation()
  const [name, setName] = useState('')

  const trimmed = name.trim()
  const canSubmit = trimmed.length > 0 && !busy

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        onOpenChange(o)
        if (!o) setName('')
      }}
    >
      <DialogContent className='sm:max-w-md'>
        <DialogHeader>
          <DialogTitle>{t('welcome.newDialogTitle', { defaultValue: 'New project' })}</DialogTitle>
          <DialogDescription>
            {t('welcome.newDialogDescription', {
              defaultValue:
                'Give your project a name. It will be stored under the managed projects directory.',
            })}
          </DialogDescription>
        </DialogHeader>
        <form
          onSubmit={(e) => {
            e.preventDefault()
            if (canSubmit) onSubmit(trimmed)
          }}
          className='flex flex-col gap-4'
        >
          <Input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder={t('welcome.newDialogPlaceholder', { defaultValue: 'Project name' })}
          />
          <DialogFooter>
            <Button type='button' variant='outline' onClick={() => onOpenChange(false)}>
              {t('common.cancel', { defaultValue: 'Cancel' })}
            </Button>
            <Button type='submit' disabled={!canSubmit}>
              <PlusIcon className='h-3.5 w-3.5' />
              {t('welcome.newDialogSubmit', { defaultValue: 'Create' })}
            </Button>
          </DialogFooter>
        </form>
      </DialogContent>
    </Dialog>
  )
}
