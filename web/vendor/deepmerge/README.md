# Vendored `deepmerge`

`@sveltejs/vite-plugin-svelte` (a build-time dependency) depends on
`deepmerge@^4.3.1`. Dependabot flags GHSA-gcqx-w48m-w2h4 (prototype poisoning in
`mergeObject()`, low, dev-only) for `>=4.2.0 <=4.3.1`, and upstream has no
fixed release. This is the 4.3.1 CommonJS build with one change: `getKeys()`
drops `__proto__`, `constructor` and `prototype`, so those keys can never be
written to the merged object regardless of the target's prototype chain.

The plugin only merges developer-authored Vite/Svelte options, so the practical
risk was low; this keeps the alert list clean without patching node_modules or
adding a runtime dependency. Delete the override in `web/package.json` once
upstream publishes a fixed `deepmerge`.

## Files

`index.js`, `index.d.ts` and `license.txt` are the readable source.
`../deepmerge-4.3.2-aiplane.1.tgz` next to this directory is what npm actually
installs (`web/package.json` depends on it and overrides the transitive
`deepmerge` with `$deepmerge`). Rebuild the tarball after editing the source:

```sh
cd web
npm pack ./vendor/deepmerge
mv deepmerge-4.3.2-aiplane.1.tgz vendor/
npm install --no-audit --no-fund
```
