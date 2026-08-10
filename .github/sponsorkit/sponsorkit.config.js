import { defineConfig } from 'sponsorkit'

if (process.env.SPONSORKIT_GITHUB_TOKEN) providers.push('github')
if (process.env.SPONSORKIT_PATREON_TOKEN) providers.push('patreon')

export default defineConfig({
  github: {
    login: 'mayocream',
    type: 'user',
  },
  outputDir: '.',
  formats: ['svg'],
  width: 800,
  onSponsorsAllFetched(sponsors) {
    let anonymousCount = 0

    return sponsors.map((sponsorship) => {
      if (sponsorship.sponsor.name || sponsorship.sponsor.login) return sponsorship

      anonymousCount += 1

      return {
        ...sponsorship,
        sponsor: {
          ...sponsorship.sponsor,
          login: `anonymous-${anonymousCount}`,
          name: 'Anonymous',
          avatarUrl: undefined,
          websiteUrl: undefined,
          linkUrl: undefined,
        },
      }
    })
  },
})
