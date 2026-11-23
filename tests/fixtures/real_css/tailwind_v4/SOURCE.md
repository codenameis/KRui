# Real CSS Fixture Provenance — Tailwind CSS v4

- **Source package**: `@tailwindcss/cli@4.3.5` + `tailwindcss@4.3.5` (npm official registry, MIT License)
- **Scanned template**: `examples/09_tailwind_v4/views/app.krui` (via `tools/tailwind_input.css`)
- **Oracle generator**: `playwright@1.65.1` headless Chromium (`tools/extract_css_oracle.mjs`)
- **Regeneration script**: `pwsh -NoProfile -File tools/regen_real_css.ps1`
- **Modification policy**: Zero manual edits; exact byte-for-byte CLI output.

## Exact Commands

``powershell
./tools/node_modules/.bin/tailwindcss.cmd --cwd tools -i tailwind_input.css -o tests/fixtures/real_css/tailwind_v4/tailwind_v4.css
./tools/node_modules/.bin/tailwindcss.cmd --cwd tools -i tailwind_input.css -o tests/fixtures/real_css/tailwind_v4/tailwind_v4.optimize.css --optimize
node tools/extract_css_oracle.mjs
``

## SHA256 Digests

| File | SHA256 |
|---|---|
| `tailwind_v4.css` | `685978484d76780d84978f2c5b0e231bc56cfaf1c1d74ae90377af5e2e6b74da` |
| `tailwind_v4.optimize.css` | `ac86e4b3c02a95f103fc51b464b80e510f9edb6f0590c429d3051063da2b271c` |
| `oracle.json` | `622cd5be6b4beccdb996fcc01983b0f36a7a0e411a4014ce8a9e77d5d95ac76f` |
