import { MonitorIcon, MoonIcon, SunIcon } from 'lucide-react'
import { SETTINGS_API_KEY_PROVIDERS } from '@/lib/features/llm/providers'
import {
  ALL_PRESETS,
  LOCAL_LLM_PRESET_DEFINITIONS,
} from '@/lib/features/llm/presets'

export const THEME_OPTIONS = [
  { value: 'light', icon: SunIcon, labelKey: 'settings.themeLight' },
  { value: 'dark', icon: MoonIcon, labelKey: 'settings.themeDark' },
  { value: 'system', icon: MonitorIcon, labelKey: 'settings.themeSystem' },
] as const

export const API_PROVIDERS = SETTINGS_API_KEY_PROVIDERS

export const PRESET_BUTTONS = ALL_PRESETS.map((preset) => ({
  value: preset,
  labelKey: LOCAL_LLM_PRESET_DEFINITIONS[preset].settingsLabelKey,
}))

export const DEFAULT_SYSTEM_PROMPT =
  'You are a professional manga translator. Translate Japanese manga dialogue into natural {target_language} that fits inside speech bubbles. Preserve character voice, emotional tone, relationship nuance, emphasis, and sound effects naturally. Keep the wording concise. Do not add notes, explanations, or romanization. If the input contains <block id="N">...</block>, translate only the text inside each block. Keep every block tag exactly unchanged, including ids, order, and block count. Do not merge blocks, split blocks, or add any text outside the blocks.'

export const inputClassName =
  'border-border bg-card text-foreground placeholder:text-muted-foreground focus:ring-primary w-full rounded-md border px-3 py-1.5 text-sm focus:ring-1 focus:outline-none'
