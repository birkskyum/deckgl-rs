# The visgl-rs site

An [Astro](https://astro.build) and [Starlight](https://starlight.astro.build) site, published
to <https://birkskyum.github.io/visgl-rs/> by `.github/workflows/site.yml` whenever
`site/` or `docs/` changes on `main`.

```sh
npm install
npm run dev       # http://localhost:4321/visgl-rs/
npm run build     # into dist/
npm run preview   # serve what was built
```

The pages under `Documentation` are the repository's own `docs/*.md`, copied in by
`scripts/sync-docs.mjs` before every build along with `docs/images`. Edit the documents in
`docs/`, not the copies: `src/content/docs/docs/` and `public/images/` are generated and not
checked in. `scripts/sync-docs.mjs` decides which documents appear and in what order, so a new
document needs an entry there. The base path the site is served from lives in `base.mjs`.
