'use client'

import type {
  CanvasState,
  Download,
  Event as AgentRunEvent,
  Job,
  LoginEvent as AgentLoginEvent,
  ModelResources,
  ProjectInfo,
  StartupState,
  UpdateProgress,
} from './protocol'
import { subscribe, type AppError, type EventGapError } from './transport'

export interface WindowState {
  maximized: boolean
  minimized: boolean
  fullscreen: boolean
  focused: boolean
}

export type AppEvent =
  | { type: 'startup_ready'; startup: StartupState }
  | { type: 'startup_failed'; error: AppError }
  | { type: 'canvas'; state: CanvasState }
  | { type: 'job'; job: Job }
  | { type: 'download'; download: Download }
  | { type: 'resources'; resources: ModelResources }
  | { type: 'project'; project: ProjectInfo | null }
  | { type: 'agent_login'; event: AgentLoginEvent }
  | { type: 'agent_run'; event: AgentRunEvent }
  | { type: 'window_state'; state: WindowState }
  | { type: 'update_progress'; progress: UpdateProgress }

export function subscribeAppEvents(
  listener: (event: AppEvent) => void,
  onGap?: (error: EventGapError) => void,
): () => void {
  return subscribe(listener, onGap)
}
