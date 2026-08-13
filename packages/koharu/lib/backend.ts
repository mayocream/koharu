'use client'

import { commands } from './protocol'
import { receiveError, receivePreferences, receiveTranslationModels } from './store'

type Command<Args extends unknown[], Result> = (...args: Args) => Promise<Result>

let translationModelsRequest: Promise<void> | null = null
let translationModelsGeneration = 0

export async function call<Args extends unknown[], Result>(
  command: Command<Args, Result>,
  ...args: Args
): Promise<Result> {
  try {
    return await command(...args)
  } catch (error) {
    throw report(error)
  }
}

export function dispatch<Args extends unknown[], Result>(
  command: Command<Args, Result>,
  ...args: Args
): void {
  void call(command, ...args).catch(() => undefined)
}

export async function refreshPreferences(): Promise<void> {
  receivePreferences(await call(commands.getPreferences))
}

export function refreshTranslationModels(force = false): Promise<void> {
  if (!force && translationModelsRequest) return translationModelsRequest

  const generation = ++translationModelsGeneration
  const request = call(commands.getTranslationModels)
    .then((models) => {
      if (generation === translationModelsGeneration) receiveTranslationModels(models)
    })
    .finally(() => {
      if (translationModelsRequest === request) translationModelsRequest = null
    })
  translationModelsRequest = request
  return request
}

export function updateViewport(element: HTMLElement): Promise<void> {
  const bounds = element.getBoundingClientRect()
  return call(
    commands.setViewport,
    bounds.x,
    bounds.y,
    bounds.width,
    bounds.height,
    window.devicePixelRatio,
    workspaceColor(),
  ).then(() => undefined)
}

function report(error: unknown): Error {
  const message =
    error instanceof Error
      ? error.message
      : typeof error === 'string'
        ? error
        : 'The native application returned an unknown error.'
  receiveError(message)
  return error instanceof Error ? error : new Error(message)
}

function workspaceColor(): [number, number, number] {
  const raw = window
    .getComputedStyle(document.documentElement)
    .getPropertyValue('--workspace-background-rgb')
  const values = raw.trim().split(/\s+/).map(Number)
  if (values.length !== 3 || values.some((value) => !Number.isFinite(value))) {
    return [183, 180, 174]
  }
  return values.map((value) => Math.min(255, Math.max(0, Math.round(value)))) as [
    number,
    number,
    number,
  ]
}
