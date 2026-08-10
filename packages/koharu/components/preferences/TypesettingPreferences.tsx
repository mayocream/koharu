'use client'

import { Plus, Trash2 } from 'lucide-react'
import { useMemo } from 'react'

import { FontPicker } from '@/components/controls/FontPicker'
import { PreferencePage } from '@/components/preferences/PreferenceFields'
import type { TypesettingConfig } from '@/lib/protocol'
import { useFonts } from '@/lib/queries'
import { Button } from '@koharu/ui/components/button'

export function TypesettingPreferences({
  value,
  onChange,
}: {
  value: TypesettingConfig
  onChange: (value: TypesettingConfig) => void
}) {
  const fonts = useFonts()
  const families = fonts.data ?? []
  const fontFamilies = value.font_families ?? []
  const selectedNames = useMemo(() => new Set(fontFamilies.map(normalizeFontName)), [fontFamilies])
  const availableFamilies = families.filter(
    (family) => !selectedNames.has(normalizeFontName(family.name)),
  )

  const replaceFamily = (index: number, family: string) => {
    const next = [...fontFamilies]
    next[index] = family
    onChange({ ...value, font_families: next })
  }

  const removeFamily = (index: number) => {
    onChange({
      ...value,
      font_families: fontFamilies.filter((_, candidate) => candidate !== index),
    })
  }

  return (
    <PreferencePage
      title='Typesetting'
      description='Set the ordered font stack used when a text layer has no assigned family.'
    >
      <section aria-labelledby='typesetting-font-stack'>
        <header className='mb-3'>
          <h3 id='typesetting-font-stack' className='text-[12px] font-semibold'>
            Font fallback
          </h3>
          <p className='mt-0.5 text-[11px] leading-4 text-muted-foreground'>
            Koharu uses the first available family that supports the text.
          </p>
        </header>
        <div className='overflow-hidden rounded-xl border border-border/80 bg-[var(--surface-panel)]'>
          {fontFamilies.length > 0 ? (
            <ol className='divide-y divide-border/80' aria-label='Default font families'>
              {fontFamilies.map((family, index) => {
                const choices = families.filter(
                  (candidate) =>
                    normalizeFontName(candidate.name) === normalizeFontName(family) ||
                    !selectedNames.has(normalizeFontName(candidate.name)),
                )
                return (
                  <li
                    key={`${normalizeFontName(family)}-${index}`}
                    className='group flex min-w-0 items-center gap-3 px-3 py-2'
                  >
                    <span
                      aria-hidden='true'
                      className='flex size-6 shrink-0 items-center justify-center rounded-md bg-muted text-[9px] font-semibold text-muted-foreground tabular-nums'
                    >
                      {String(index + 1).padStart(2, '0')}
                    </span>
                    <div className='min-w-0 flex-1'>
                      <FontPicker
                        value={family}
                        families={choices}
                        ariaLabel={`Default font family ${index + 1}`}
                        onChange={(next) => replaceFamily(index, next)}
                      />
                    </div>
                    <Button
                      type='button'
                      variant='ghost'
                      size='icon-sm'
                      aria-label={`Remove ${family}`}
                      title={`Remove ${family}`}
                      className='shrink-0 text-muted-foreground hover:bg-destructive/10 hover:text-destructive'
                      onClick={() => removeFamily(index)}
                    >
                      <Trash2 className='size-3.5' />
                    </Button>
                  </li>
                )
              })}
            </ol>
          ) : (
            <p role='status' className='px-4 py-5 text-[11px] text-muted-foreground'>
              No default families. Koharu will use the system fallback.
            </p>
          )}
          <div className='flex min-w-0 items-center gap-3 border-t border-border/80 bg-muted/20 px-3 py-2'>
            <span className='flex size-6 shrink-0 items-center justify-center text-muted-foreground'>
              <Plus className='size-3.5' />
            </span>
            <div className='min-w-0 flex-1'>
              <FontPicker
                value=''
                families={availableFamilies}
                disabled={fonts.isPending || availableFamilies.length === 0}
                ariaLabel='Add default font family'
                placeholder={fonts.isPending ? 'Loading fonts...' : 'Add font family'}
                onChange={(family) =>
                  onChange({ ...value, font_families: [...fontFamilies, family] })
                }
              />
            </div>
            <span aria-hidden='true' className='size-7 shrink-0' />
          </div>
        </div>
        {fonts.isError && (
          <p role='status' className='mt-2 text-[10px] leading-4 text-destructive'>
            Fonts could not be loaded. Existing families can still be removed.
          </p>
        )}
      </section>
    </PreferencePage>
  )
}

function normalizeFontName(value: string): string {
  return value.normalize('NFKC').trim().replace(/\s+/g, ' ').toLowerCase()
}
