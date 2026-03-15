/**
 * Plays a "Ding Ding" notification sound using the Web Audio API.
 */
export function playDingDing() {
  if (typeof window === 'undefined') return

  const AudioContext = window.AudioContext || (window as any).webkitAudioContext
  const ctx = new AudioContext()

  function playTone(freq: number, start: number, duration: number) {
    const osc = ctx.createOscillator()
    const gain = ctx.createGain()

    osc.type = 'sine'
    osc.frequency.setValueAtTime(freq, start)
    osc.frequency.exponentialRampToValueAtTime(freq * 0.5, start + duration)

    gain.gain.setValueAtTime(0.2, start)
    gain.gain.exponentialRampToValueAtTime(0.01, start + duration)

    osc.connect(gain)
    gain.connect(ctx.destination)

    osc.start(start)
    osc.stop(start + duration)
  }

  // Double ding
  playTone(880, ctx.currentTime, 0.4) // A5
  playTone(880, ctx.currentTime + 0.15, 0.4) // A5
}
