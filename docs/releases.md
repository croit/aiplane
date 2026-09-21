# Releases & versioning

How this project is versioned and how a release is cut. **Authority for the
version scheme** — the same scheme croit ERP uses, so a croit engineer reads
both the same way.

---

## Version scheme — `YYMM.RELEASE.BUILD`

Date-based (you read "when" straight off the number) and still valid SemVer, so
Helm, Docker tags and every tool that expects three numbers keep working.

```
2609 . 1 . 0
│      │   └─ BUILD    0 on a tagged release; on main builds = the number of
│      │              commits since the last release tag (automatic, monotonic)
│      └──── RELEASE   release counter within the YYMM line; the first release
│                      of a month is 1
└─────────── YYMM      2-digit year + 2-digit month (2026-09 → 2609)
```

| Version      | Meaning                                                  |
|--------------|----------------------------------------------------------|
| `2609.1.0`   | Tagged release, September 2026, first release that month |
| `2609.1.7`   | Main build, 7 commits after `v2609.1.0`                  |
| `2609.2.0`   | Next tagged release in the same month                    |
| `2610.1.0`   | First tagged release in October 2026                     |
| `2609.0.306` | Pre-release line — this repository has never been tagged |

**The collision-free rule:** a release **always** ends in `.0`, and a dev build
**always** has BUILD ≥ 1 (it has at least one commit past the tag). So a dev
build's number can never equal a release's. To cut a release you bump
`RELEASE` (`2609.1.0` → `2609.2.0`) or roll `YYMM` into the current month
(`2610.1.0`). Never reuse a number.

`RELEASE` counts within its `YYMM` line and starts at 1 each month.

**`RELEASE = 0` is reserved** for a repository that has not cut its first
release yet. Until the first tag exists, builds are named `YYMM.0.<commits>` —
still monotonic, still collision-free against every real release.

---

## Where the version comes from

**The git tag is the only source of truth.** Nothing reads `Cargo.toml`'s
`version` to identify a build.

| Component | Role |
|---|---|
| [`scripts/derive-version.sh`](../scripts/derive-version.sh) | The single resolver. Prints the version for any build. |
| `.github/workflows/ci.yml` → `version` job | Runs it once per pipeline and hands the result to every other job, so no two artifacts can disagree. |
| `Dockerfile` (`ARG`/`ENV AIPLANE_VERSION`) | Stamps it into the gateway image as an environment variable — not compiled in. |
| `crates/aiplane-api/src/build_info.rs` | Reads `$AIPLANE_VERSION` at runtime, falls back to the crate version. |
| `GET /api/v0/build` → `version` | What the UI's source/version line shows. |
| Image tags | Every build publishes `:v<version>`; see the tag table below. |
| Helm chart (`--version` / `--app-version`) | A published chart carries the same number and therefore pins `image.tag: v<version>` by default. |
| `Cargo.toml` `version` | **Nothing.** It is not read by the build, the image tags, or the chart. It still feeds `CARGO_PKG_VERSION` in outbound user-agent strings; leave it where it is and do not spend a commit on it. |

### Resolution order

1. **Tag build** (`GITHUB_REF_TYPE=tag`) → the tag is authoritative, minus its
   `v`. A tag that is not `vX.Y.Z` is a hard error.
2. **A reachable `v*.*.*` tag** → `git describe --long` →
   `MAJOR.MINOR.<commits-since-tag>`.
3. **No `v*.*.*` tag anywhere in the repository** → `YYMM.0.<commits>`, with a
   warning. This is the pre-first-release line.
4. **Tags exist but none is reachable** (a shallow clone) → **hard error.**

Case 4 is deliberate. A build that cannot see the tags is a misconfigured
build, and a made-up number would mislabel an image that nobody can later trace
back. There is no `Cargo.toml` fallback for the same reason: it would have to be
bumped by hand, and when it drifted it would fail silently instead of loudly.

> **CI requirement:** the `version` job checks out with `fetch-depth: 0`. Without
> it, `git describe` sees nothing and the pipeline fails instead of shipping a
> wrong number.
>
> **Caller requirement:** in a `set -e` shell, assign and then export
> separately. `V=$(sh scripts/derive-version.sh)` aborts on failure;
> `export V=$(...)` does **not** — `export`'s own exit status wins — and would
> carry on with an empty version.

Locally:

```bash
mise run version        # or: sh scripts/derive-version.sh
```

---

## What a build publishes

Every push to `main` and every release tag publishes four images and one chart,
all stamped with the same number:

| Artifact | Image / reference |
|---|---|
| Gateway | `ghcr.io/croit/aiplane` |
| Sandbox gold image | `ghcr.io/croit/aiplane-sandbox` |
| Sandbox runner | `ghcr.io/croit/aiplane-sandbox-runner` |
| OCR sidecar | `ghcr.io/croit/aiplane-ocr-sidecar` |
| Helm chart | `oci://ghcr.io/croit/charts/aiplane` |

A **tag** build makes one more thing: a [GitHub Release][gh-releases] entry,
written by the `github release` job once every publishing job above it has
succeeded. That ordering is the point — the Releases page is what a human reads
instead of checking GHCR by hand, so it must never be able to name a build
whose images did not actually ship. The entry is a face on the tag, nothing
more: its notes are the commit range since the previous release, nothing reads
it, and deleting it would not un-publish a single image.

[gh-releases]: https://github.com/croit/aiplane/releases

### Tag ownership

Every moving tag has exactly one owning job, so two pipelines can never race
for it:

| Tag | Written by | Meaning |
|---|---|---|
| `:v2609.1.0` | every build | Immutable pin of one build. A chart pins this. |
| `:sha-<sha>` | every build | Immutable pin of one commit. |
| `:latest` | main builds | Rolling latest `main`. Dev/staging convenience. |
| `:production` | tag builds only | Latest official release — the production deploy target. |
| `:<branch>` | branch builds | Convenience pointer at a branch's last build. |

**Deploy `:production` or an explicit `:v2609.1.0`, never `:latest`.**

### Channels — what an installation actually follows

The chart is published to the same registry, in two channels:

| Published from | Chart version | Who gets it |
|---|---|---|
| a release tag | `2609.1.0` | Everyone. `helm install` with **no** `--version` resolves to the newest of these. |
| `main` | `2609.1.7-main.abc1234` | Nobody by accident: it is a SemVer **prerelease**, and Helm skips prereleases when it resolves "latest". Reachable with `--devel` or an exact `--version`. |

That is verified behaviour, not an assumption: against a registry holding
`1.0.0`, `1.0.1` and `1.1.0-main.42`, `helm show chart` with no `--version`
returns `1.0.1`, and `--devel` returns `1.1.0-main.42`.

A released chart's `appVersion` is the plain version, and the chart derives its
image tags from it, so chart and images always belong to the same build.

What the chart then pulls depends on one value:

| `autoUpdate.enabled` | Image tag | Meaning |
|---|---|---|
| `true` (default) | `:production` with `pullPolicy: Always` | Follow the newest release. A CronJob restarts the pod nightly so a moved tag is picked up. |
| `false` | `:v2609.1.0` with `IfNotPresent` | Exactly the build this chart was released with. |
| (git checkout) | `:latest` with `Always` | The rolling `main` image — for development. |

So the default is "stay on the latest release without doing anything", and
pinning is one value away. The operational caveats of that default — a
single-replica restart, and migrations applying unattended — are spelled out in
[`kubernetes.md`](kubernetes.md#updates).

---

## Cutting a release

> 🔴 **Never cut a release unprompted.** "Ship X" or "finish X" is not a release
> request. A release happens when someone explicitly asks for one.

A release is **a tag and nothing else.** There is no version commit to make,
nothing to bump, and no file to edit.

**1. Decide the number.** It is derived, not chosen:

```bash
git fetch --tags --quiet
YYMM=$(date +%y%m)
NEXT=$(git tag -l "v$YYMM.*" --sort=-v:refname | head -1 \
        | sed -E "s/^v$YYMM\.([0-9]+)\..*/\1/")
echo "v$YYMM.$(( ${NEXT:-0} + 1 )).0"
```

Sanity rules — a violation means the computation was wrong, so recompute:

- `YYMM` is **today's** month. Never a future one: "this release is for
  October" is not a reason to tag `2610` in September.
- Never reuse or skip a `RELEASE` number within a line.
- The result must not already exist (`git tag -l 'v<computed>'` is empty).

**2. Tag a commit whose CI is already green**, annotated and `v`-prefixed:

```bash
git tag -a v2609.1.0 -m "Release 2609.1.0"   # on the green commit
git push origin v2609.1.0
```

**3. Watch the tag pipeline.** It runs the *full* suite again — lint, tests,
release build — and only then publishes. So unlike a promotion-style pipeline,
a red tag build publishes nothing; the tests are the gate, not a prior run's
receipt. Last, after every image and the chart are up, it creates the GitHub
Release entry. There is nothing to write by hand.

The practical consequence is worth knowing: the released image is **rebuilt**,
not relabelled, so it is not bit-for-bit the image `main` built from the same
commit. Tagging a commit whose `main` run was green is therefore about avoiding
surprises, not about the release inheriting its proof. (The version itself is a
runtime `ENV`, not compiled in, so moving to a promote-the-tested-image model
later would not need a code change.)

**4. Verify** that `:v2609.1.0` and `:production` exist for all four images and
that the chart resolves — with no `--version`, which is what every installation
and every auto-update follows:

```bash
helm show chart oci://ghcr.io/croit/charts/aiplane
gh release view v2609.1.0        # the entry, and that its notes read sensibly
```

Note what step 3 implies for users: publishing a release **moves
`:production`**, so every installation with auto-updates on picks it up at its
next scheduled restart. A release is not a quiet event.

After this, every subsequent main build labels itself `2609.1.1`, `2609.1.2`, …
with no further action.

---

## Gotchas

- **A tag pipeline failed — can I re-tag?** Delete it locally and remotely
  (`git tag -d v2609.1.0 && git push origin :refs/tags/v2609.1.0`), fix, re-tag.
  Only safe before anyone pulled the images; otherwise cut the next number.
- **Why does a fresh tag build show `2609.1.0` and not `2609.1.something`?** On
  the tagged commit, commits-since-tag is 0 anyway, and the tag short-circuits
  the resolver.
- **`cargo run` locally shows `v0.1.0`.** `AIPLANE_VERSION` is unset outside a
  CI-built image, so AIplane falls back to the crate version. Expected.
- **A tag that is not `vX.Y.Z`** does not start a build at all: the workflow
  only triggers on `v*`, and the resolver rejects anything else.
- **The Releases page is empty for an old release.** Entries are only created
  from `v2609.2.0` onward; earlier tags predate the `github release` job and
  were never backfilled. The tag is the release either way.
- **Editing release notes is safe.** They are prose for humans — no tooling
  parses them, and the images and chart are already published by the time the
  entry exists.
- **Never hand-edit a version** in `Chart.yaml`, the Dockerfile or a CI job. The
  number flows from the tag through `derive-version.sh` and nowhere else.

---

## Related

- [`kubernetes.md`](kubernetes.md) — installing the published chart
- [`../deploy/README.md`](../deploy/README.md) — every image and what it is for
- [`../scripts/derive-version.sh`](../scripts/derive-version.sh) — the resolver itself
