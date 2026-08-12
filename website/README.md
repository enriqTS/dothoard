# dothoard documentation site

This Astro Starlight site is published with GitHub Pages. Markdown in `../docs/`
is the single source of truth: `npm run build` copies it into Starlight's content
directory before validating and building the site. Do not edit generated files
under `src/content/docs/` other than `index.md`.

```bash
cd website
npm install
npm run dev
npm run build
```

The deployment workflow uses `/dothoard` as its default GitHub Pages base path.
Set the `BASE` environment variable when testing a different project path or a
custom domain.
