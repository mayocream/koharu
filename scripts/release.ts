import { exec as execCallback } from 'node:child_process'
import { readFile, writeFile } from 'node:fs/promises'
import { promisify } from 'node:util'
import path from 'node:path'

const exec = promisify(execCallback)
const root = path.resolve(__dirname, '..')
const execOpts = { cwd: root, maxBuffer: 10 * 1024 * 1024 }

async function main() {
  let bumpedVersion = process.argv[2]?.trim()

  if (bumpedVersion) {
    console.log(`Using provided version: ${bumpedVersion}`)
  } else {
    console.log('Calculating bumped version with git-cliff...')
    bumpedVersion = (
      await exec('bun git-cliff --unreleased --bumped-version', execOpts)
    ).stdout.trim()
  }

  if (!bumpedVersion) {
    throw new Error('git-cliff did not return a bumped version')
  }

  console.log(`Bumped version: ${bumpedVersion}`)

  const cargoTomlPath = path.join(root, 'Cargo.toml')
  const cargoToml = await readFile(cargoTomlPath, 'utf8')
  const versionPattern =
    /(\[workspace\.package\][\s\S]*?version\s*=\s*")([^"]+)(")/

  if (!versionPattern.test(cargoToml)) {
    throw new Error('Could not find [workspace.package] version in Cargo.toml')
  }

  const updatedCargoToml = cargoToml.replace(
    versionPattern,
    `$1${bumpedVersion}$3`,
  )
  await writeFile(cargoTomlPath, updatedCargoToml)
  console.log('Updated Cargo.toml version')

  await exec('cargo metadata --format-version 1', execOpts)
  console.log('Updated Cargo.lock')

  await exec('git add Cargo.toml Cargo.lock', execOpts)
  await exec(`git commit -m "chore(release): ${bumpedVersion}"`, execOpts)
  console.log('Created release commit')

  await exec(`git tag ${bumpedVersion}`, execOpts)
  console.log('Created git tag')

  await exec(`bun git-cliff -o CHANGELOG.md`, execOpts)
  console.log('Updated CHANGELOG.md')

  await exec('git add CHANGELOG.md', execOpts)
  await exec(`git commit --amend --no-edit`, execOpts)
  console.log('Amended release commit with updated CHANGELOG.md')

  await exec(`git tag -f ${bumpedVersion}`, execOpts)
  console.log('Updated git tag to include CHANGELOG.md')

  console.log(`Release commit and tag ${bumpedVersion} created.`)
}

main().catch((error) => {
  console.error(error)
  process.exit(1)
})
