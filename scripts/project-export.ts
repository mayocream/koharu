import { mkdir, readFile, stat, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { spawn, type ChildProcessWithoutNullStreams } from 'node:child_process'

type ExportFormat = 'rendered' | 'inpainted' | 'psd'

type Options = {
  apiBase?: string
  binaryPath?: string
  format: ExportFormat
  keepRunning: boolean
  outputDir: string
  port: number
  project?: string
}

type ProjectSummary = {
  id: string
  name: string
  currentDocumentId: string | null
}

type DocumentSummary = {
  id: string
  name: string
}

function usage() {
  console.log(`Usage:
  bun run project:export -- --output <dir> [--project <id-or-path>] [--format rendered|inpainted|psd]
  bun run project:psd -- --output <dir> [--project <id-or-path>]

Options:
  --output <dir>     Output directory for exported files
  --project <value>  Project id, project root, or project_manifest.json path
  --format <value>   rendered | inpainted | psd (default: rendered)
  --port <value>     Headless API port (default: 9998)
  --binary <path>    Override the Koharu binary path
  --api-base <url>   Use an existing API base instead of spawning a headless process
  --keep-running     Do not stop the spawned headless process when export finishes
  --help             Show this help
`)
}

function parseArgs(argv: string[]): Options {
  const options: Options = {
    format: 'rendered',
    keepRunning: false,
    outputDir: '',
    port: 9998,
  }

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index]

    switch (arg) {
      case '--help':
      case '-h':
        usage()
        process.exit(0)
      case '--output':
        options.outputDir = argv[++index] ?? ''
        break
      case '--project':
        options.project = argv[++index]
        break
      case '--format': {
        const value = argv[++index]
        if (value !== 'rendered' && value !== 'inpainted' && value !== 'psd') {
          throw new Error(`Unsupported format: ${value ?? '<missing>'}`)
        }
        options.format = value
        break
      }
      case '--port':
        options.port = Number(argv[++index] ?? '')
        if (!Number.isInteger(options.port) || options.port <= 0) {
          throw new Error(`Invalid port: ${argv[index] ?? '<missing>'}`)
        }
        break
      case '--binary':
        options.binaryPath = argv[++index]
        break
      case '--api-base':
        options.apiBase = argv[++index]
        break
      case '--keep-running':
        options.keepRunning = true
        break
      default:
        throw new Error(`Unknown argument: ${arg}`)
    }
  }

  if (!options.outputDir) {
    throw new Error('Missing required --output <dir>')
  }

  return options
}

function defaultBinaryPath() {
  return process.platform === 'win32'
    ? path.resolve('target/release/koharu.exe')
    : path.resolve('target/release/koharu')
}

async function pathExists(target: string) {
  try {
    await stat(target)
    return true
  } catch {
    return false
  }
}

async function nextAvailablePath(targetPath: string) {
  if (!(await pathExists(targetPath))) {
    return targetPath
  }

  const parsed = path.parse(targetPath)
  let suffix = 2

  while (true) {
    const candidate = path.join(
      parsed.dir,
      `${parsed.name}_${suffix}${parsed.ext}`,
    )
    if (!(await pathExists(candidate))) {
      return candidate
    }
    suffix += 1
  }
}

async function resolveProjectId(projectArg: string) {
  const resolved = path.resolve(projectArg)
  if (!(await pathExists(resolved))) {
    return projectArg
  }

  const stats = await stat(resolved)
  const manifestPath = stats.isDirectory()
    ? path.join(resolved, 'project_manifest.json')
    : resolved

  const manifest = JSON.parse(await readFile(manifestPath, 'utf8')) as {
    id?: string
  }
  if (!manifest.id) {
    throw new Error(`Project manifest did not contain an id: ${manifestPath}`)
  }
  return manifest.id
}

async function fetchJson<T>(
  apiBase: string,
  apiPath: string,
  init?: RequestInit,
) {
  const response = await fetch(`${apiBase}${apiPath}`, init)
  if (!response.ok) {
    const message = await response.text()
    throw new Error(message || `${response.status} ${response.statusText}`)
  }
  return (await response.json()) as T
}

async function fetchBinary(apiBase: string, apiPath: string) {
  const response = await fetch(`${apiBase}${apiPath}`)
  if (!response.ok) {
    const message = await response.text()
    throw new Error(message || `${response.status} ${response.statusText}`)
  }

  return {
    data: Buffer.from(await response.arrayBuffer()),
    filename: parseFilename(response.headers.get('content-disposition')),
  }
}

function parseFilename(contentDisposition: string | null) {
  if (!contentDisposition) return undefined

  const utf8Match = contentDisposition.match(/filename\*=UTF-8''([^;]+)/i)
  if (utf8Match?.[1]) {
    return decodeURIComponent(utf8Match[1])
  }

  const quotedMatch = contentDisposition.match(/filename="([^"]+)"/i)
  if (quotedMatch?.[1]) {
    return quotedMatch[1]
  }

  const plainMatch = contentDisposition.match(/filename=([^;]+)/i)
  return plainMatch?.[1]?.trim()
}

function fallbackFilename(document: DocumentSummary, format: ExportFormat) {
  if (format === 'psd') {
    return `${document.name}_koharu.psd`
  }

  const suffix = format === 'rendered' ? 'koharu' : 'inpainted'
  return `${document.name}_${suffix}.png`
}

function spawnHeadless(binaryPath: string, port: number) {
  const child = spawn(binaryPath, ['--headless', '--port', String(port)], {
    cwd: process.cwd(),
    windowsHide: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  })

  let logs = ''
  const appendLogs = (chunk: Buffer) => {
    logs += chunk.toString()
    if (logs.length > 8000) {
      logs = logs.slice(-8000)
    }
  }

  child.stdout.on('data', appendLogs)
  child.stderr.on('data', appendLogs)

  return {
    child,
    logs: () => logs.trim(),
  }
}

async function waitForApi(
  apiBase: string,
  child?: ChildProcessWithoutNullStreams,
  logs?: () => string,
) {
  const deadline = Date.now() + 120_000

  while (Date.now() < deadline) {
    if (child?.exitCode != null) {
      throw new Error(
        `Headless Koharu exited before the API became ready.\n${logs?.() ?? ''}`.trim(),
      )
    }

    try {
      const response = await fetch(`${apiBase}/meta`)
      if (response.ok) {
        return
      }
    } catch {}

    await new Promise((resolve) => setTimeout(resolve, 1000))
  }

  throw new Error('Timed out while waiting for the Koharu API')
}

async function main() {
  const options = parseArgs(process.argv.slice(2))
  const apiBase =
    options.apiBase?.replace(/\/$/, '') ??
    `http://127.0.0.1:${options.port}/api/v1`

  let spawned:
    | {
        child: ChildProcessWithoutNullStreams
        logs: () => string
      }
    | undefined

  if (!options.apiBase) {
    const binaryPath = path.resolve(options.binaryPath ?? defaultBinaryPath())
    if (!(await pathExists(binaryPath))) {
      throw new Error(`Koharu binary not found: ${binaryPath}`)
    }
    spawned = spawnHeadless(binaryPath, options.port)
  }

  try {
    await waitForApi(apiBase, spawned?.child, spawned?.logs)

    if (options.project) {
      const projectId = await resolveProjectId(options.project)
      await fetchJson(
        apiBase,
        `/projects/${encodeURIComponent(projectId)}/open`,
        {
          method: 'POST',
        },
      )
    }

    const project = await fetchJson<ProjectSummary | null>(
      apiBase,
      '/projects/current',
    )
    if (!project) {
      throw new Error(
        'No current project is open. Pass --project <id-or-path>.',
      )
    }

    const documents = await fetchJson<DocumentSummary[]>(apiBase, '/documents')
    if (!documents.length) {
      throw new Error(`Project "${project.name}" has no pages to export`)
    }

    const outputDir = path.resolve(options.outputDir)
    await mkdir(outputDir, { recursive: true })

    for (const document of documents) {
      const apiPath =
        options.format === 'psd'
          ? `/documents/${document.id}/export/psd`
          : `/documents/${document.id}/export?layer=${options.format}`
      const result = await fetchBinary(apiBase, apiPath)
      const filename =
        result.filename ?? fallbackFilename(document, options.format)
      const outputPath = await nextAvailablePath(path.join(outputDir, filename))
      await writeFile(outputPath, result.data)
      console.log(`Wrote ${outputPath}`)
    }

    console.log(
      `Exported ${documents.length} page(s) from project "${project.name}" to ${outputDir}`,
    )
  } finally {
    if (spawned && !options.keepRunning) {
      spawned.child.kill()
    }
  }
}

main().catch((error) => {
  process.stderr.write(`Error: ${(error as Error).message}\n`)
  process.exit(1)
})
