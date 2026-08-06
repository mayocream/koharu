'use client'

import { Folder, Plus, Settings, Trash2 } from 'lucide-react'
import Image from 'next/image'
import { useCallback, useEffect, useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'

import { call } from '@/lib/backend'
import { commands, type ProjectSummary } from '@/lib/protocol'
import { useKoharuStore } from '@/lib/store'
import { Button } from '@koharu/ui/components/button'
import { Input } from '@koharu/ui/components/input'

export function StartView() {
  const { t } = useTranslation()
  const setSettingsOpen = useKoharuStore((state) => state.setSettingsOpen)
  const [projects, setProjects] = useState<ProjectSummary[]>([])
  const [name, setName] = useState('')
  const [busy, setBusy] = useState<string | null>('list')

  const reload = useCallback(async () => {
    setBusy('list')
    try {
      setProjects(await call(commands.listProjects))
    } finally {
      setBusy(null)
    }
  }, [])

  useEffect(() => {
    void reload().catch(() => undefined)
  }, [reload])

  const createProject = async (event: FormEvent) => {
    event.preventDefault()
    const projectName = name.trim()
    if (!projectName || busy) return
    setBusy('create')
    try {
      await call(commands.createProject, projectName)
      setName('')
    } finally {
      setBusy(null)
    }
  }

  const openProject = async (projectName: string) => {
    if (busy) return
    setBusy(projectName)
    try {
      await call(commands.openProject, projectName)
    } finally {
      setBusy(null)
    }
  }

  const deleteProject = async (projectName: string) => {
    if (busy || !window.confirm(`Delete “${projectName}” and all of its pages?`)) return
    setBusy(projectName)
    try {
      await call(commands.deleteProject, projectName)
      await reload()
    } finally {
      setBusy(null)
    }
  }

  return (
    <main className='grid min-h-0 flex-1 place-items-center overflow-auto bg-[var(--surface-canvas)] px-4 py-6 sm:px-8'>
      <section className='w-full max-w-[520px]' aria-labelledby='start-title'>
        <header className='flex items-center gap-2.5 px-1'>
          <span className='grid size-9 shrink-0 place-items-center rounded-xl bg-accent'>
            <Image src='/icon.png' alt='' width={19} height={19} draggable={false} priority />
          </span>
          <div className='min-w-0'>
            <h1 className='text-[13px] font-semibold tracking-[-0.02em]'>Koharu</h1>
            <p className='text-[10px] text-muted-foreground'>Manga translation workspace</p>
          </div>
          <Button
            type='button'
            variant='ghost'
            size='sm'
            className='ml-auto h-8 gap-1.5 px-2.5 text-[10px] text-muted-foreground hover:bg-foreground/[0.05] hover:text-foreground'
            onClick={() => setSettingsOpen(true)}
          >
            <Settings className='size-3' />
            {t('native.menu.settings', { defaultValue: 'Settings' })}
          </Button>
        </header>

        <div className='mt-4 overflow-hidden rounded-2xl bg-[var(--surface-panel)] shadow-[var(--shadow-panel)]'>
          <div className='border-b border-border/70 px-5 py-4'>
            <h2 id='start-title' className='text-[18px] font-semibold tracking-[-0.025em]'>
              Projects
            </h2>
            <p className='mt-1 text-[11px] leading-4 text-muted-foreground'>
              Projects stay in your Documents folder and are managed by Koharu.
            </p>
          </div>

          <form className='flex gap-2 border-b border-border/70 p-3' onSubmit={createProject}>
            <Input
              value={name}
              onChange={(event) => setName(event.target.value)}
              placeholder='Project name'
              aria-label='Project name'
              autoComplete='off'
              disabled={busy !== null}
            />
            <Button type='submit' size='sm' className='gap-1.5' disabled={!name.trim() || busy !== null}>
              <Plus className='size-3.5' />
              Create
            </Button>
          </form>

          <div className='max-h-72 min-h-28 overflow-y-auto p-2' aria-busy={busy === 'list'}>
            {busy === 'list' && projects.length === 0 ? (
              <p className='grid min-h-24 place-items-center text-[11px] text-muted-foreground'>
                Loading projects…
              </p>
            ) : projects.length === 0 ? (
              <div className='grid min-h-24 place-items-center text-center' role='status'>
                <div>
                  <Folder className='mx-auto size-5 text-muted-foreground/70' />
                  <p className='mt-2 text-[11px] font-medium'>No projects yet</p>
                  <p className='mt-0.5 text-[10px] text-muted-foreground'>Create one above to begin.</p>
                </div>
              </div>
            ) : (
              <ul className='grid gap-1' aria-label='Projects'>
                {projects.map((project) => (
                  <li key={project.name} className='group flex items-center rounded-lg hover:bg-accent/60'>
                    <button
                      type='button'
                      className='flex min-w-0 flex-1 items-center gap-2.5 px-2.5 py-2 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring'
                      onClick={() => void openProject(project.name).catch(() => undefined)}
                      disabled={busy !== null}
                    >
                      <Folder className='size-4 shrink-0 text-muted-foreground' />
                      <span className='truncate text-[11px] font-medium'>{project.name}</span>
                    </button>
                    <Button
                      type='button'
                      size='icon-sm'
                      variant='ghost'
                      className='mr-1 size-7 text-muted-foreground opacity-0 group-hover:opacity-100 focus-visible:opacity-100'
                      aria-label={`Delete ${project.name}`}
                      onClick={() => void deleteProject(project.name).catch(() => undefined)}
                      disabled={busy !== null}
                    >
                      <Trash2 className='size-3.5' />
                    </Button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        </div>
      </section>
    </main>
  )
}
