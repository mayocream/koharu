'use client'

// Just use native fetch - no special handling needed
export const fetch = globalThis.fetch
export async function initFetch(): Promise<void> {}
