import {
  getConfig as getConfigRemote,
  getMeta as getMetaRemote,
  initializeSystem as initializeSystemRemote,
  listFonts as listFontsRemote,
  updateConfig as updateConfigRemote,
} from '@/lib/generated/orval/system/system'
import type {
  BootstrapConfig,
  FontFaceInfo,
  MetaInfo,
} from '@/lib/contracts/protocol'

export const getRemoteBootstrapConfig = async () =>
  (await getConfigRemote()) as BootstrapConfig

export const getConfig = getRemoteBootstrapConfig

export const updateRemoteBootstrapConfig = async (config: BootstrapConfig) =>
  (await updateConfigRemote(config)) as BootstrapConfig

export const updateConfig = updateRemoteBootstrapConfig

export const initializeRemoteSystem = async () => await initializeSystemRemote()

export const initializeSystem = initializeRemoteSystem

export const getRemoteMeta = async () => (await getMetaRemote()) as MetaInfo

export const getMeta = getRemoteMeta

export const listRemoteFonts = async () =>
  (await listFontsRemote()) as FontFaceInfo[]

export const listFonts = listRemoteFonts
