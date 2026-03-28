import os from 'node:os'
import path from 'node:path'
import { readdir, access } from 'node:fs/promises'
import { exec as execCallback, spawn } from 'node:child_process'
import { promisify } from 'node:util'

const exec = promisify(execCallback)
const VISUAL_STUDIO_ROOTS = [
  'C:/Program Files (x86)/Microsoft Visual Studio',
  'C:/Program Files/Microsoft Visual Studio',
]
const VISUAL_STUDIO_EDITIONS = [
  'BuildTools',
  'Community',
  'Professional',
  'Enterprise',
  'Preview',
]

async function pathExists(target: string) {
  try {
    await access(target)
    return true
  } catch {
    return false
  }
}

async function checkNvcc() {
  const cudaPath = process.env.CUDA_PATH
  if (cudaPath) {
    const nvccPath = path.join(cudaPath, 'bin', 'nvcc.exe')
    if (await pathExists(nvccPath)) {
      process.env.PATH = `${path.join(cudaPath, 'bin')}${path.delimiter}${process.env.PATH}`
      return
    }
  }

  try {
    await exec('nvcc --version', { env: process.env })
  } catch {
    throw new Error('nvcc not found')
  }
}

function sortVersionsDesc(versions: string[]) {
  return versions.sort((a, b) =>
    b.localeCompare(a, undefined, { numeric: true, sensitivity: 'base' }),
  )
}

async function setupCuda() {
  const cudaPath = process.env.CUDA_PATH
  if (cudaPath) {
    const binPath = path.join(cudaPath, 'bin')
    process.env.PATH = `${binPath}${path.delimiter}${process.env.PATH}`
    return
  }

  const cudaRoot = 'C:/Program Files/NVIDIA GPU Computing Toolkit/CUDA'
  const versions = await readdir(cudaRoot).catch(() => [])

  sortVersionsDesc(versions)

  for (const version of versions) {
    if (version.startsWith('v')) {
      const binPath = path.join(cudaRoot, version, 'bin')
      if (await pathExists(binPath)) {
        process.env.PATH = `${binPath}${path.delimiter}${process.env.PATH}`
        process.env.CUDA_PATH = path.join(cudaRoot, version)

        console.log(`Added CUDA to PATH: ${binPath}`)
        return
      }
    }
  }

  throw new Error(
    'NVCC not found. Please install the CUDA Toolkit from https://developer.nvidia.com/cuda-downloads',
  )
}

async function findVcVarsWithVsWhere() {
  for (const root of VISUAL_STUDIO_ROOTS) {
    const vswherePath = path.join(root, 'Installer/vswhere.exe')
    if (!(await pathExists(vswherePath))) {
      continue
    }

    const { stdout } = await exec(
      `"${vswherePath}" -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`,
    )
    const installPath = stdout
      .split(/\r?\n/)
      .map((line) => line.trim())
      .find(Boolean)

    if (!installPath) {
      continue
    }

    const vcvarsPath = path.join(installPath, 'VC/Auxiliary/Build/vcvars64.bat')
    if (await pathExists(vcvarsPath)) {
      return vcvarsPath
    }
  }

  return null
}

async function findVcVarsWithFilesystem() {
  for (const root of VISUAL_STUDIO_ROOTS) {
    const vsVersions = await readdir(root).catch(() => [])
    for (const vsVersion of sortVersionsDesc(vsVersions)) {
      for (const edition of VISUAL_STUDIO_EDITIONS) {
        const vcvarsPath = path.join(
          root,
          vsVersion,
          edition,
          'VC/Auxiliary/Build/vcvars64.bat',
        )
        if (await pathExists(vcvarsPath)) {
          return vcvarsPath
        }
      }
    }
  }

  return null
}

async function setupCl() {
  const vcvarsPath =
    (await findVcVarsWithVsWhere()) ?? (await findVcVarsWithFilesystem())
  if (!vcvarsPath) {
    throw new Error(
      'cl.exe not found. Please install Visual Studio with C++ build tools from https://visualstudio.microsoft.com/downloads/',
    )
  }

  const { stdout } = await exec(`cmd /d /s /c ""${vcvarsPath}" >nul && set"`, {
    env: process.env,
    maxBuffer: 16 * 1024 * 1024,
  })
  for (const line of stdout.split(/\r?\n/)) {
    const separatorIndex = line.indexOf('=')
    if (separatorIndex <= 0) {
      continue
    }
    const key = line.slice(0, separatorIndex)
    const value = line.slice(separatorIndex + 1)
    process.env[key] = value
  }

  console.log(`Loaded MSVC environment from: ${vcvarsPath}`)
}

function quoteArgument(arg: string) {
  if (!/\s/.test(arg) && !/[&()^|<>"]/.test(arg)) {
    return arg
  }
  return `"${arg.replace(/"/g, '\\"')}"`
}

function buildCommand(args: string[]) {
  return args.map(quoteArgument).join(' ')
}

async function runCommand(args: string[]) {
  const command = buildCommand(args)
  const proc = spawn(command, {
    stdio: 'inherit',
    shell: true,
    env: process.env,
  })

  proc.on('error', (err) => {
    throw err
  })

  proc.on('exit', (code) => {
    process.exit(code ?? 1)
  })
}

async function ensureWindowsToolchain() {
  await checkNvcc()
    .catch(async () => {
      await setupCuda()
      await checkNvcc()
    })

  await setupCl()
}

async function dev() {
  if (os.type() === 'Windows_NT') {
    await ensureWindowsToolchain()
  }

  const args = process.argv.slice(2)
  if (args.length === 0) {
    throw new Error('No command provided')
  }

  await runCommand(args)
}

dev().catch((err) => {
  process.stderr.write(`Error: ${err.message} \n`)
  process.exit(1)
})
