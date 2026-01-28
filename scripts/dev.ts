import os from 'node:os'
import path from 'node:path'
import { readdir, access } from 'node:fs/promises'
import { exec as execCallback, spawn } from 'node:child_process'
import { promisify } from 'node:util'

const exec = promisify(execCallback)

async function pathExists(target: string) {
  try {
    await access(target)
    return true
  } catch {
    return false
  }
}

async function checkNvcc() {
  try {
    await exec('nvcc --version', { env: process.env })
  } catch {
    throw new Error('nvcc not found')
  }
}

function sortVersionsDesc(versions: string[]) {
  return versions.sort((a, b) => {
    const verA = parseInt(a.replace('v', '').replace('.', ''))
    const verB = parseInt(b.replace('v', '').replace('.', ''))
    return verB - verA
  })
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
    'NVCC not found. Please install the CUDA Toolkit from https://developer.nvidia.com/cuda-12-9-1-download-archive',
  )
}

async function setupCudnn() {
  const cudnnRoot = 'C:/Program Files/NVIDIA/CUDNN'
  const versions = await readdir(cudnnRoot).catch(() => [])

  sortVersionsDesc(versions)

  for (const version of versions) {
    if (version.startsWith('v')) {
      const binPath = path.join(cudnnRoot, version, 'bin')

      if (await pathExists(binPath)) {
        const versions = await readdir(binPath)

        sortVersionsDesc(versions)

        for (const version of versions) {
          const fullPath = path.join(binPath, version)
          process.env.PATH = `${fullPath}${path.delimiter}${process.env.PATH}`

          console.log(`Added cuDNN to PATH: ${fullPath}`)
          return
        }
      }
    }

    throw new Error(
      'cuDNN not found. Please install cuDNN from https://developer.nvidia.com/rdp/cudnn-download',
    )
  }
}

async function setupCl() {
  const vsRoot = 'C:/Program Files/Microsoft Visual Studio'
  const vsVersions = await readdir(vsRoot).catch(() => [])

  for (const vsVersion of vsVersions) {
    const vcPath = path.join(vsRoot, vsVersion, 'Community/VC/Tools/MSVC')
    if (await pathExists(vcPath)) {
      const msvcVersions = await readdir(vcPath)
      for (const msvcVersion of msvcVersions) {
        const binPath = path.join(vcPath, msvcVersion, 'bin/Hostx64/x64')
        if (await pathExists(binPath)) {
          process.env.PATH = `${binPath}${path.delimiter}${process.env.PATH}`

          console.log(`Added cl.exe to PATH: ${binPath}`)
          return
        }
      }
    }
  }

  throw new Error(
    'cl.exe not found. Please install Visual Studio with C++ build tools from https://visualstudio.microsoft.com/downloads/',
  )
}

function getCargoFeatures(): string[] {
  const platform = os.type()
  if (platform === 'Windows_NT' || platform === 'Linux') {
    return ['cuda', 'cudnn']
  } else if (platform === 'Darwin') {
    return ['metal']
  }
  return []
}

function injectCargoFeatures(args: string[]): string[] {
  // Check if this is a cargo command
  if (args[0] !== 'cargo') {
    return args
  }

  // Don't add features if --features is already specified
  if (args.includes('--features')) {
    return args
  }

  const features = getCargoFeatures()
  if (features.length === 0) {
    return args
  }

  // Insert --features after cargo subcommand (run, build, etc.)
  const result = [...args]
  // Find position after subcommand (cargo run, cargo build, etc.)
  if (result.length >= 2) {
    result.splice(2, 0, '--features', features.join(','))
  }

  return result
}

async function dev() {
  if (os.type() === 'Windows_NT') {
    // First, try to check if nvcc is available
    await checkNvcc()
      // If not found, try to set up CUDA paths
      .catch(async () => {
        await setupCuda()
        // Check again after setup
        await checkNvcc()
      })

    // Setup cuDNN path
    await setupCudnn()

    // Setup cl.exe path
    await setupCl()
  }

  let args = process.argv.slice(2)
  if (args.length === 0) {
    throw new Error('No command provided')
  }

  // Inject cargo features based on platform
  args = injectCargoFeatures(args)

  console.log(`Running: ${args.join(' ')}`)

  const proc = spawn(args.join(' '), {
    stdio: 'inherit',
    shell: true,
    env: process.env,
  })

  proc.on('error', (err) => {
    throw err
  })

  proc.on('exit', (code) => {
    process.exit(code ?? 0)
  })
}

dev().catch((err) => {
  process.stderr.write(`Error: ${err.message} \n`)
  process.exit(1)
})
