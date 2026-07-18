'use client'

import { FilePlus2, FolderOpen, PlugZap } from 'lucide-react'
import { AnimatePresence, motion, useReducedMotion } from 'motion/react'
import Image from 'next/image'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { koharuClient } from '@/lib/koharu'

const SOURCE_GLYPHS = [...'吾輩は猫である。名前はまだない']
const FINAL_TEXT = "I am a cat. I don't have a name yet."
const PHASE_DELAYS = [2200, 1800, 1400, 3200]

export function WelcomeScreen({ disconnected = false }: { disconnected?: boolean }) {
  const { t } = useTranslation()

  return (
    <main className='flex min-h-0 flex-1 items-start justify-center overflow-y-auto bg-background'>
      <section className='flex w-full max-w-md flex-col px-6 pt-14 pb-10 sm:pt-20'>
        <header className='flex flex-col items-center text-center'>
          <Image src='/icon.png' alt='Koharu' width={52} height={52} draggable={false} priority />
          <h1 className='mt-4 text-2xl font-semibold tracking-tight'>
            {t('native.welcome.title', { defaultValue: 'Manga translation workspace' })}
          </h1>
          <p className='mt-1.5 max-w-sm text-xs leading-5 text-muted-foreground'>
            {t('native.welcome.description', {
              defaultValue: 'Clean, translate, and typeset every page in one focused workspace.',
            })}
          </p>
        </header>

        {disconnected && (
          <div className='mt-5 flex gap-2.5 rounded-md border border-amber-500/30 bg-amber-500/8 px-3 py-2.5 text-xs'>
            <PlugZap className='mt-0.5 size-3.5 shrink-0 text-amber-600' />
            <div>
              <p className='font-medium'>
                {t('native.welcome.bridgeMissing', { defaultValue: 'Native bridge unavailable' })}
              </p>
              <p className='mt-0.5 leading-4 text-muted-foreground'>
                {t('native.welcome.bridgeHelp', {
                  defaultValue:
                    'This standalone browser view is intentionally read-only. Start the Rust desktop host to create or open projects.',
                })}
              </p>
            </div>
          </div>
        )}

        <WorkflowPreview />

        <div className='mt-5 grid grid-cols-2 gap-2.5'>
          <Button
            className='h-10'
            disabled={disconnected}
            onClick={() => koharuClient.fire({ type: 'create_project' })}
          >
            <FilePlus2 />
            {t('native.welcome.newProject', { defaultValue: 'New Project' })}
          </Button>
          <Button
            className='h-10'
            variant='outline'
            disabled={disconnected}
            onClick={() => koharuClient.fire({ type: 'open_project' })}
          >
            <FolderOpen />
            {t('native.welcome.openProject', { defaultValue: 'Open Project' })}
          </Button>
        </div>
      </section>
    </main>
  )
}

function WorkflowPreview() {
  const { t } = useTranslation()
  const reduceMotion = useReducedMotion()
  const [phase, setPhase] = useState(reduceMotion ? 3 : 0)
  const [typedCharacters, setTypedCharacters] = useState(reduceMotion ? FINAL_TEXT.length : 0)
  const steps = [
    t('native.stage.detection', { defaultValue: 'Detection' }),
    t('native.stage.segmentation', { defaultValue: 'Segmentation' }),
    t('native.stage.inpainting', { defaultValue: 'Inpainting' }),
    t('native.stage.typography', { defaultValue: 'Typography' }),
  ]

  useEffect(() => {
    if (reduceMotion) {
      setPhase(3)
      return
    }
    const timer = window.setTimeout(
      () => setPhase((current) => (current + 1) % steps.length),
      PHASE_DELAYS[phase],
    )
    return () => window.clearTimeout(timer)
  }, [phase, reduceMotion, steps.length])

  useEffect(() => {
    if (reduceMotion) {
      setTypedCharacters(FINAL_TEXT.length)
      return
    }
    if (phase !== 3) {
      setTypedCharacters(0)
      return
    }
    const timer = window.setInterval(() => {
      setTypedCharacters((current) => {
        if (current >= FINAL_TEXT.length) {
          window.clearInterval(timer)
          return current
        }
        return current + 1
      })
    }, 62)
    return () => window.clearInterval(timer)
  }, [phase, reduceMotion])

  return (
    <figure
      role='img'
      aria-label={t('native.welcome.workflow', { defaultValue: 'How Koharu works' })}
      className='mx-auto mt-7 w-full max-w-[330px]'
    >
      <div className='relative aspect-[247/112] overflow-hidden rounded-md border bg-muted/20'>
        <span className='absolute top-4 bottom-4 left-1/2 w-px bg-border/45' aria-hidden='true' />
        <span className='absolute top-1/2 right-6 left-6 h-px bg-border/45' aria-hidden='true' />

        <div className='absolute inset-0 flex items-center justify-center'>
          {phase <= 2 && (
            <div className='grid grid-cols-8 gap-x-1 gap-y-1.5' lang='ja'>
              {SOURCE_GLYPHS.map((glyph, index) => (
                <span
                  key={`${glyph}-${index}`}
                  className='relative flex h-8 w-7 items-center justify-center'
                >
                  <motion.span
                    animate={{
                      opacity: phase === 2 ? 0 : 1,
                    }}
                    transition={{
                      duration: reduceMotion ? 0 : 0.24,
                    }}
                    className={`font-serif text-[19px] leading-none font-medium transition-colors duration-300 ${phase === 1 ? 'text-violet-500' : 'text-foreground'}`}
                  >
                    {glyph}
                  </motion.span>

                  <AnimatePresence>
                    {phase === 0 && (
                      <motion.span
                        initial={{ opacity: 0 }}
                        animate={{ opacity: 1 }}
                        exit={{ opacity: 0 }}
                        transition={{
                          duration: reduceMotion ? 0 : 0.18,
                        }}
                        className='absolute inset-0 border border-primary/80'
                      />
                    )}
                  </AnimatePresence>
                </span>
              ))}
            </div>
          )}

          {phase === 3 && (
            <motion.div
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              transition={{ duration: reduceMotion ? 0 : 0.16 }}
              className='flex h-12 items-center text-[17px] font-medium tracking-tight text-foreground'
              lang='en'
              style={{ fontFamily: 'var(--font-literata), Georgia, serif' }}
            >
              <span>{FINAL_TEXT.slice(0, typedCharacters)}</span>
              {!reduceMotion && typedCharacters < FINAL_TEXT.length && (
                <motion.span
                  animate={{ opacity: [1, 0] }}
                  transition={{ duration: 0.4, repeat: Infinity, repeatType: 'reverse' }}
                  className='ml-0.5 inline-block h-5 w-px bg-foreground'
                />
              )}
            </motion.div>
          )}
        </div>

        <span className='absolute top-3 left-3 size-1.5 border-t border-l border-foreground/35' />
        <span className='absolute right-3 bottom-3 size-1.5 border-r border-b border-foreground/35' />
      </div>

      <figcaption className='mt-2'>
        <div className='flex items-center gap-2'>
          <span className='font-mono text-[9px] text-muted-foreground'>0{phase + 1}</span>
          <AnimatePresence mode='wait' initial={false}>
            <motion.span
              key={steps[phase]}
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: reduceMotion ? 0 : 0.16 }}
              className='text-[10px] font-medium text-muted-foreground'
            >
              {steps[phase]}
            </motion.span>
          </AnimatePresence>
          <span className='h-px flex-1 bg-border' />
          <div className='flex gap-1' aria-hidden='true'>
            {steps.map((step, index) => (
              <span
                key={step}
                className={`size-1 rounded-full ${index === phase ? 'bg-foreground' : 'bg-border'}`}
              />
            ))}
          </div>
        </div>
      </figcaption>
    </figure>
  )
}
