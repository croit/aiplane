#!/usr/bin/env bash
# SPDX-License-Identifier: AGPL-3.0-only
# Copyright (C) 2026 croit GmbH
#
# Run one local command under a resident-memory ceiling.
#
# macOS cannot enforce RLIMIT_AS/RLIMIT_RSS (setrlimit returns EINVAL), and a
# build or a parallel test run that outgrows the host takes the whole machine
# with it. This wrapper puts the command in its own process group, samples the
# group's RSS once a second, and terminates it just before the ceiling. The
# default is 32 GiB; override with MEMORY_LIMIT_GB.
#
# This is a development tool, not part of the gateway: it exists because the
# alternative is a crashed machine and lost work. Every local build, lint,
# test or browser run goes through `mise run guard -- <command>`.
set -euo pipefail

if [[ "${1:-}" =~ ^[0-9]+$ ]]; then
	limit_gb="$1"
	shift
else
	limit_gb="${MEMORY_LIMIT_GB:-32}"
fi
if [ "$limit_gb" -lt 1 ]; then
	echo "memory-guard: the limit must be a positive integer (got '$limit_gb')" >&2
	exit 2
fi
limit_kb=$((limit_gb * 1024 * 1024))

if [ "$#" -eq 0 ]; then
	echo "memory-guard: no command given" >&2
	echo "usage: scripts/memory-guard.sh <command> [args...]" >&2
	exit 2
fi

# Job control gives the command its own process group, so one signal reaches
# every compiler, test binary and browser it spawns.
set -m
"$@" &
child=$!
pgid="$(ps -o pgid= -p "$child" 2>/dev/null | tr -d ' ')" || pgid="$child"
[ -n "$pgid" ] || pgid="$child"

terminate() {
	kill -TERM -- "-$pgid" 2>/dev/null || true
	sleep 2
	kill -KILL -- "-$pgid" 2>/dev/null || true
}
trap 'terminate; exit 130' INT TERM

peak_kb=0
while :; do
	# A finished child stays visible as a zombie until `wait`, so the state
	# check ends the loop; `kill -0` alone would spin here.
	state="$(ps -o state= -p "$child" 2>/dev/null | tr -d ' ')" || true
	[ -z "$state" ] && break
	case "$state" in Z*) break ;; esac

	rss_kb="$(ps -axo pgid=,rss= | awk -v pgid="$pgid" '$1 == pgid { sum += $2 } END { print sum + 0 }')" || true
	[ "$rss_kb" -gt "$peak_kb" ] && peak_kb="$rss_kb"
	if [ "$rss_kb" -gt "$limit_kb" ]; then
		echo "memory-guard: process group $pgid reached $((rss_kb / 1024)) MiB, over the ${limit_gb} GiB ceiling; terminating it" >&2
		terminate
		wait "$child" 2>/dev/null || true
		exit 137
	fi
	sleep 1
done

set +e
wait "$child"
status=$?
set -e
if [ "${MEMORY_GUARD_REPORT:-0}" = "1" ]; then
	echo "memory-guard: peak $((peak_kb / 1024)) MiB (limit ${limit_gb} GiB)" >&2
fi
exit "$status"
