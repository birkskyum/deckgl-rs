// @ts-check
import { defineConfig } from 'astro/config'
import starlight from '@astrojs/starlight'
import { base } from './base.mjs'

// The site is served from GitHub Pages at /visgl-rs
export default defineConfig({
  site: 'https://birkskyum.github.io',
  base,
  integrations: [
    starlight({
      title: 'visgl-rs',
      description:
        'A native implementation of the vis.gl stack: Rust on wgpu, rendering with deck.gl’s own WGSL shaders and taking Apache Arrow and GeoArrow data directly.',
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/birkskyum/visgl-rs',
        },
      ],
      editLink: {
        baseUrl: 'https://github.com/birkskyum/visgl-rs/edit/main/site/',
      },
      customCss: ['./src/styles/site.css'],
      sidebar: [
        { label: 'Overview', link: '/' },
        { label: 'Gallery', link: '/gallery/' },
        { label: 'Documentation', items: [{ autogenerate: { directory: 'docs' } }] },
        // rustdoc, built by .github/workflows/site.yml and copied into dist/api
        { label: 'API reference', link: '/api/' },
      ],
    }),
  ],
})
