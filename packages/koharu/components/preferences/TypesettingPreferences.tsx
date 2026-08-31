'use client'

import { Plus, Trash2 } from 'lucide-react'
import { useMemo } from 'react'
import { useTranslation } from 'react-i18next'

import { FontPicker } from '@/components/controls/FontPicker'
import {
  PreferencePage,
  PreferenceRow,
  PreferenceSection,
} from '@/components/preferences/PreferenceFields'
import { useFonts } from '@/lib/queries'
import type { TypesettingConfig } from '@koharu/bridge/protocol'
import { Button } from '@koharu/ui/components/button'

export function TypesettingPreferences({
  value,
  onChange,
}: {
  value: TypesettingConfig
  onChange: (value: TypesettingConfig) => void
}) {
  const { t } = useTranslation()
  const fonts = useFonts()
  const families = fonts.data ?? []
  const fontFamilies = useMemo(() => value.font_families ?? [], [value.font_families])
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
      title={t('settings.typesetting.title')}
      description={t('settings.typesetting.description')}
    >
      <section aria-labelledby='typesetting-font-stack'>
        <header className='mb-3'>
          <h3 id='typesetting-font-stack' className='text-[12px] font-semibold'>
            {t('settings.typesetting.fontFallback')}
          </h3>
          <p className='mt-0.5 text-[11px] leading-4 text-muted-foreground'>
            {t('settings.typesetting.fontFallbackDescription')}
          </p>
        </header>
        <div className='overflow-hidden rounded-xl border border-border/80 bg-[var(--surface-panel)]'>
          {fontFamilies.length > 0 ? (
            <ol
              className='divide-y divide-border/80'
              aria-label={t('settings.typesetting.defaultFamilies')}
            >
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
                        ariaLabel={t('settings.typesetting.familyLabel', { index: index + 1 })}
                        onChange={(next) => replaceFamily(index, next)}
                      />
                    </div>
                    <Button
                      type='button'
                      variant='ghost'
                      size='icon-sm'
                      aria-label={t('settings.typesetting.removeFamily', { family })}
                      title={t('settings.typesetting.removeFamily', { family })}
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
              {t('settings.typesetting.empty')}
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
                ariaLabel={t('settings.typesetting.addFamily')}
                placeholder={
                  fonts.isPending
                    ? t('settings.typesetting.loadingFonts')
                    : t('settings.typesetting.addFamily')
                }
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
            {t('settings.typesetting.loadError')}
          </p>
        )}
      </section>

      {}
      <PreferenceSection
        title={t('settings.typesetting.overrides.title')}
        description={t('settings.typesetting.overrides.description')}
      >
        <PreferenceRow
          title={t('settings.typesetting.overrides.fontColor')}
          description={t('settings.typesetting.overrides.fontColorDescription')}
        >
          <Button
            type='button'
            variant={value.force_black_text ? 'default' : 'outline'}
            className='h-8 text-[11px]'
            onClick={() =>
              onChange({
                ...value,
                force_black_text: !value.force_black_text,
              })
            }
          >
            {value.force_black_text ? t('settings.typesetting.overrides.revertAuto') : t('settings.typesetting.overrides.forceBlack')}
          </Button>
        </PreferenceRow>

        <PreferenceRow
          title={t('settings.typesetting.overrides.borderWidth')}
          description={t('settings.typesetting.overrides.borderWidthDescription')}
        >
          <div className='flex items-center gap-2'>
            {value.force_border_width !== null && value.force_border_width !== undefined && (
              <div className='flex items-center gap-1.5'>
                <input
                  type='number'
                  min='0'
                  step='0.1'
                  className='h-8 w-16 rounded-md border border-input bg-transparent px-2 py-1 text-right text-[11px] tabular-nums shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring'
                  value={value.force_border_width}
                  onChange={(e) => {
                    const num = parseFloat(e.target.value)
                    onChange({ ...value, force_border_width: isNaN(num) ? 0 : num })
                  }}
                />
                <span className='select-none text-[11px] text-muted-foreground'>px</span>
              </div>
            )}
            <Button
              type='button'
              variant={
                value.force_border_width !== null && value.force_border_width !== undefined
                  ? 'default'
                  : 'outline'
              }
              className='h-8 text-[11px]'
              onClick={() =>
                onChange({
                  ...value,
                  force_border_width:
                    value.force_border_width !== null && value.force_border_width !== undefined
                      ? null
                      : 0.5,
                })
              }
            >
              {value.force_border_width !== null && value.force_border_width !== undefined
                ? t('settings.typesetting.overrides.revert')
                : t('settings.typesetting.overrides.override')}
            </Button>
          </div>
        </PreferenceRow>

        <PreferenceRow
          title={t('settings.typesetting.overrides.fontWeight')}
          description={t('settings.typesetting.overrides.fontWeightDescription')}
        >
          <div className='flex items-center gap-2'>
            {value.force_font_weight !== null && value.force_font_weight !== undefined && (
              <input
                type='number'
                min='100'
                max='900'
                step='100'
                className='h-8 w-16 rounded-md border border-input bg-transparent px-2 py-1 text-right text-[11px] tabular-nums shadow-sm transition-colors focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-ring'
                value={value.force_font_weight}
                onChange={(e) => {
                  const num = parseInt(e.target.value, 10)
                  onChange({ ...value, force_font_weight: isNaN(num) ? 400 : num })
                }}
              />
            )}
            <Button
              type='button'
              variant={
                value.force_font_weight !== null && value.force_font_weight !== undefined
                  ? 'default'
                  : 'outline'
              }
              className='h-8 text-[11px]'
              onClick={() =>
                onChange({
                  ...value,
                  force_font_weight:
                    value.force_font_weight !== null && value.force_font_weight !== undefined
                      ? null
                      : 400,
                })
              }
            >
              {value.force_font_weight !== null && value.force_font_weight !== undefined
                ? t('settings.typesetting.overrides.revert')
                : t('settings.typesetting.overrides.override')}
            </Button>
          </div>
        </PreferenceRow>
      </PreferenceSection>
    </PreferencePage>
  )
}

function normalizeFontName(value: string): string {
  return value.normalize('NFKC').trim().replace(/\s+/g, ' ').toLowerCase()
}
