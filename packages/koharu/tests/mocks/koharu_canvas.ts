function unavailable(): never {
  throw new Error('The generated WebAssembly canvas is unavailable in UI tests.')
}

export default async function initialize(): Promise<never> {
  return unavailable()
}

export async function createCanvas(): Promise<never> {
  return unavailable()
}
