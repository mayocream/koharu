import path from 'node:path'
import { mkdir } from 'node:fs/promises'
import { spawn } from 'node:child_process'
import { fileURLToPath } from 'node:url'

const uiRoot = path.resolve(fileURLToPath(new URL('..', import.meta.url)))
const repoRoot = path.resolve(uiRoot, '..')
const generatedRoot = path.join(uiRoot, '.generated')
const specPath = path.join(generatedRoot, 'openapi.json')
const orvalConfigPath = path.join(uiRoot, 'orval.config.ts')

const run = async (command: string, args: string[], cwd: string) => {
  await new Promise<void>((resolve, reject) => {
    const child = spawn(command, args, {
      cwd,
      env: process.env,
      stdio: 'inherit',
    })

    child.on('error', reject)
    child.on('exit', (code) => {
      if (code === 0) {
        resolve()
        return
      }

      reject(new Error(`${command} exited with code ${code ?? 1}`))
    })
  })
}

await mkdir(generatedRoot, { recursive: true })

await run(
  'cargo',
  ['run', '-q', '-p', 'koharu-rpc', '--bin', 'export-openapi', '--', specPath],
  repoRoot,
)

await run('bunx', ['orval', '--config', orvalConfigPath], uiRoot)
