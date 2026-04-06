'use client'

import { useCallback } from 'react'
import { usePreferencesStore } from '@/lib/stores/preferencesStore'

export function useTextToSpeech() {
  const enabled = usePreferencesStore((s) => s.ttsEnabled)
  const rate = usePreferencesStore((s) => s.ttsRate)

  const speak = useCallback(
    (text: string) => {
      if (!enabled || !text) return
      speechSynthesis.cancel()
      // Strip punctuation (CJK and common)
      const cleaned = text.replace(
        /[。、！？「」『』（）【】〈〉《》…～·\u3000.,!?'"()\[\]{}<>:;\-—–_\s]+/g,
        ' ',
      ).trim()
      if (!cleaned) return
      const utterance = new SpeechSynthesisUtterance(cleaned)
      utterance.lang = 'zh-HK'
      utterance.rate = rate
      speechSynthesis.speak(utterance)
    },
    [enabled, rate],
  )

  const stop = useCallback(() => {
    speechSynthesis.cancel()
  }, [])

  return {
    speak,
    stop,
    isSupported: typeof speechSynthesis !== 'undefined',
  }
}
