import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'OpenBPM',
  description: 'High-accuracy BPM detection. Pure Rust. Multi-estimator fusion and learned judge router.',
  base: '/open-bpm/',
  cleanUrls: true,
  lastUpdated: true,
  sitemap: {
    hostname: 'https://fabriziosalmi.github.io/open-bpm/',
  },

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/open-bpm/favicon.svg' }],
    ['meta', { name: 'theme-color', content: '#10b981' }],
    ['meta', { name: 'color-scheme', content: 'dark light' }],
    ['meta', { property: 'og:type', content: 'website' }],
    ['meta', { property: 'og:title', content: 'OpenBPM — High-Accuracy BPM Detection in Pure Rust' }],
    [
      'meta',
      {
        property: 'og:description',
        content:
          'High-accuracy BPM detection library and CLI. Runs 6 independent estimators in parallel, fused with metrical clustering and a learned judge router.',
      },
    ],
    ['meta', { property: 'og:url', content: 'https://fabriziosalmi.github.io/open-bpm/' }],
    ['meta', { name: 'twitter:card', content: 'summary' }],
    ['meta', { name: 'twitter:title', content: 'OpenBPM — High-Accuracy BPM Detection in Rust' }],
    [
      'meta',
      {
        name: 'twitter:description',
        content: 'Pure Rust BPM detection library and CLI. 6 estimators, metrical fusion, octave resolution.',
      },
    ],
    ['meta', { name: 'robots', content: 'index, follow, max-image-preview:large' }],
  ],

  themeConfig: {
    siteTitle: 'OpenBPM',

    nav: [
      { text: 'Guide', link: '/guide/introduction', activeMatch: '/guide/' },
      { text: 'Specification', link: '/guide/specification' },
      { text: 'Benchmarks', link: '/guide/benchmarks' },
      { text: 'Architecture', link: '/guide/architecture' },
      { text: 'GitHub', link: 'https://github.com/fabriziosalmi/open-bpm' },
    ],

    sidebar: [
      {
        text: 'Overview & Guide',
        items: [
          { text: 'Introduction', link: '/guide/introduction' },
          { text: 'Installation & CLI Usage', link: '/guide/installation' },
        ],
      },
      {
        text: 'Technical Documentation',
        items: [
          { text: 'Algorithm Specification', link: '/guide/specification' },
          { text: 'Benchmark & Accuracy', link: '/guide/benchmarks' },
          { text: 'State Machine & Roadmap', link: '/guide/architecture' },
        ],
      },
    ],

    socialLinks: [
      { icon: 'github', link: 'https://github.com/fabriziosalmi/open-bpm' },
    ],

    search: {
      provider: 'local',
      options: {
        detailedView: true,
      },
    },

    outline: {
      level: [2, 3],
      label: 'On this page',
    },

    footer: {
      message: 'Pure Rust BPM detection · Released under the MIT License.',
      copyright: 'Copyright © Fabrizio Salmi',
    },

    docFooter: {
      prev: 'Previous',
      next: 'Next',
    },
  },

  markdown: {
    theme: {
      light: 'github-light',
      dark: 'github-dark',
    },
  },
})
