'use client'

export type Platform = 'mac' | 'windows' | 'linux' | 'unknown'

let cachedPlatform: Platform | null = null

export function getPlatform(): Platform {
  if (cachedPlatform) return cachedPlatform
  if (typeof navigator === 'undefined') return 'unknown'
  const nav = navigator as any
  const platform = (nav.userAgentData?.platform || nav.platform || '').toLowerCase()
  if (platform.includes('mac')) cachedPlatform = 'mac'
  else if (platform.includes('win')) cachedPlatform = 'windows'
  else if (platform.includes('linux')) cachedPlatform = 'linux'
  else cachedPlatform = 'unknown'

  return cachedPlatform
}

export interface ShortcutEvent {
  key: string
  ctrlKey: boolean
  metaKey: boolean
  altKey: boolean
  shiftKey: boolean
}

const MODIFIER_NAMES = new Set([
  'Control',
  'Alt',
  'Shift',
  'Meta',
  'Process',
  'AltGraph',
  'CapsLock',
  'OS',
  'Command',
])

/**
 * Generates a standardized shortcut string from a keyboard event.
 * Format: [Ctrl+][Alt/Opt+][Shift+][Cmd/Win+]Key
 */
export function formatShortcut(event: ShortcutEvent, isMac: boolean): string {
  const parts: string[] = []

  // Consistent order for internal string storage and matching
  if (event.ctrlKey) parts.push('Ctrl')
  if (event.altKey) parts.push(isMac ? 'Opt' : 'Alt')
  if (event.shiftKey) parts.push('Shift')
  if (event.metaKey) parts.push(isMac ? 'Cmd' : 'Win')

  const key = event.key

  // Skip if only a modifier key is being pressed
  if (MODIFIER_NAMES.has(key)) {
    return ''
  }

  // Standardize single characters to uppercase (V, B, [)
  const displayKey = key.length === 1 ? key.toUpperCase() : key
  parts.push(displayKey)

  return parts.join('+')
}

const BLOCKED_KEYS = new Set([
  'Escape',
  'Tab',
  'CapsLock',
  'Meta',
  'ContextMenu',
  'ScrollLock',
  'NumLock',
  'Pause',
  'Insert',
])

/**
 * Checks if a key is in the blocked list (F-keys, system keys, etc.)
 */
export function isKeyBlocked(key: string): boolean {
  const isFunctionKey = key.startsWith('F') && key.length > 1 && !isNaN(Number(key.slice(1)))

  return isFunctionKey || BLOCKED_KEYS.has(key)
}

/**
 * Performs a highly efficient shallow comparison between two shortcut objects.
 */
export function areShortcutsEqual(a: Record<string, string>, b: Record<string, string>): boolean {
  const keysA = Object.keys(a)
  const keysB = Object.keys(b)

  if (keysA.length !== keysB.length) return false

  for (let i = 0; i < keysA.length; i++) {
    const key = keysA[i]
    if (a[key] !== b[key]) return false
  }

  return true
}

/**
 * Checks if a key is a modifier key (Control, Alt, Shift, etc.).
 */
export function isModifierKey(key: string): boolean {
  return MODIFIER_NAMES.has(key)
}
