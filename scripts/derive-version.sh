#!/bin/sh
# SPDX-License-Identifier: AGPL-3.0-only
# Copyright (C) 2026 croit GmbH
#
# Derive the version string for a build. One resolver, used by every CI job
# that stamps a version onto an artifact (container images, the Helm chart) so
# they can never disagree.
#
# Scheme: YYMM.RELEASE.BUILD  (e.g. 2609.1.0)
#   YYMM     2-digit year + 2-digit month, carried by the git tag
#   RELEASE  release counter within that YYMM line; the first release of a
#            month is .1
#   BUILD    0 on a tagged release; otherwise the number of commits since the
#            last release tag, so a main build reads 2609.1.7 ("7 commits past
#            v2609.1.0") and increases monotonically.
#
# That is what makes the numbers collision-free: a release ALWAYS ends in .0,
# a dev build ALWAYS has BUILD >= 1 (it has at least one commit past the tag).
#
# The git tag is the only source of truth. Cargo.toml's `version` is not read
# here and does not identify a release — see docs/releases.md.
#
# Resolution order:
#   1. a tag build (GITHUB_REF_TYPE=tag)   -> the tag is authoritative
#   2. a reachable v*.*.* tag              -> MAJOR.MINOR.<commits-since-tag>
#   3. no v*.*.* tag exists in the repo    -> YYMM.0.<commits>, the pre-release
#      line. RELEASE=0 is reserved for "this repository has not cut its first
#      release yet", so it can never collide with a real release (>= .1).
#   4. tags exist but none is reachable    -> hard error. That is a shallow
#      clone, i.e. a misconfigured build, and emitting a made-up number would
#      mislabel an image nobody could trace back.
#
# Callers in a `set -e` shell must assign and then export separately:
# `V=$(sh scripts/derive-version.sh)` aborts on failure, while
# `export V=$(...)` does NOT (export's own exit status wins) and would carry on
# with an empty version.
#
# POSIX sh on purpose — it runs in CI containers without bash.
set -e

if [ "${GITHUB_REF_TYPE:-}" = "tag" ] && [ -n "${GITHUB_REF_NAME:-}" ]; then
    case "$GITHUB_REF_NAME" in
        v*.*.*)
            echo "${GITHUB_REF_NAME#v}"
            exit 0
            ;;
        *)
            echo "ERROR: tag '$GITHUB_REF_NAME' does not match vYYMM.RELEASE.BUILD." >&2
            echo "Release tags are annotated and v-prefixed, e.g. v2609.1.0." >&2
            exit 1
            ;;
    esac
fi

DESC=$(git describe --tags --long --match 'v*.*.*' 2>/dev/null || echo "")
if [ -n "$DESC" ]; then
    BASE=${DESC#v}      # 2609.1.0-7-gabc1234
    TAG=${BASE%%-*}     # 2609.1.0
    REST=${BASE#*-}     # 7-gabc1234
    COMMITS=${REST%%-*} # 7
    MAJOR=$(echo "$TAG" | cut -d. -f1)
    MINOR=$(echo "$TAG" | cut -d. -f2)
    echo "${MAJOR}.${MINOR}.${COMMITS}"
    exit 0
fi

# A shallow clone has no tags but is not tag-free — telling the two apart is
# what keeps case 3 from silently mislabelling a real build.
if [ "$(git rev-parse --is-shallow-repository 2>/dev/null || echo true)" = "true" ]; then
    echo "ERROR: shallow clone — no v*.*.* tag is reachable from HEAD." >&2
    echo "CI jobs that stamp a version must check out with fetch-depth: 0;" >&2
    echo "locally run 'git fetch --tags'. Refusing to emit a made-up version." >&2
    exit 1
fi

if [ -z "$(git tag -l 'v*.*.*' 2>/dev/null)" ]; then
    COMMITS=$(git rev-list --count HEAD)
    echo "WARNING: no release tag in this repository yet — using the pre-release" >&2
    echo "line YYMM.0.<commits>. Cut the first release with a v-prefixed tag." >&2
    echo "$(date -u +%y%m).0.${COMMITS}"
    exit 0
fi

echo "ERROR: v*.*.* tags exist but none is reachable from HEAD." >&2
echo "Fetch them ('git fetch --tags') or check out a commit on a released line." >&2
exit 1
