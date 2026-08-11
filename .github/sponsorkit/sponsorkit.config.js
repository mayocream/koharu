import { defineConfig } from 'sponsorkit'

const atlasCloud = {
  name: 'direct',
  async fetchSponsors() {
    return [
      {
        sponsor: {
          type: 'Organization',
          login: 'atlas-cloud',
          name: 'Atlas Cloud',
          avatarUrl: 'https://github.com/AtlasCloudAI.png?size=180',
          websiteUrl: 'https://www.atlascloud.ai/',
          linkUrl: 'https://www.atlascloud.ai/',
        },
        monthlyDollars: 150,
        provider: 'direct',
      },
    ]
  },
}

export default defineConfig({
  github: {
    login: 'mayocream',
    type: 'user',
  },
  outputDir: '.',
  formats: ['svg'],
  providers: [atlasCloud, 'github', 'patreon'],
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
