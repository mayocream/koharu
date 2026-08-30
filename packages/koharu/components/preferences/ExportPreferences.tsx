'use client'

import { useTranslation } from 'react-i18next'

import {
  NumberField,
  PreferencePage,
  PreferenceRow,
  PreferenceSection,
} from '@/components/preferences/PreferenceFields'
import { defaultExportQuality } from '@/lib/export'
import type { ExportConfig } from '@koharu/bridge/protocol'

export function ExportPreferences({
  value,
  onChange,
}: {
  value: ExportConfig
  onChange: (value: ExportConfig) => void
}) {
  const { t } = useTranslation()

  return (
    <PreferencePage
      title={t('settings.export.title')}
      description={t('settings.export.description')}
    >
      <PreferenceSection
        title={t('settings.export.quality')}
        description={t('settings.export.qualityDescription')}
      >
        <PreferenceRow
          title={t('settings.export.jpegQuality')}
          description={t('settings.export.jpegQualityDescription')}
        >
          <NumberField
            label={t('settings.export.qualityLabel')}
            value={value.jpeg_quality ?? defaultExportQuality.jpeg}
            min={1}
            max={100}
            step={1}
            onChange={(quality) =>
              onChange({ ...value, jpeg_quality: quality ?? defaultExportQuality.jpeg })
            }
          />
        </PreferenceRow>
        <PreferenceRow
          title={t('settings.export.webpQuality')}
          description={t('settings.export.webpQualityDescription')}
        >
          <NumberField
            label={t('settings.export.qualityLabel')}
            value={value.webp_quality ?? defaultExportQuality.webp}
            min={1}
            max={100}
            step={1}
            onChange={(quality) =>
              onChange({ ...value, webp_quality: quality ?? defaultExportQuality.webp })
            }
          />
        </PreferenceRow>
      </PreferenceSection>
    </PreferencePage>
  )
}
