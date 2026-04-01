import type { BootstrapConfig } from '@/lib/contracts/protocol'
import { initializeSystem, updateConfig } from '@/lib/infra/system/api'

export const saveBootstrapConfig = async (config: BootstrapConfig) =>
  await updateConfig(config)

export const startSystemInitialization = async () => await initializeSystem()
