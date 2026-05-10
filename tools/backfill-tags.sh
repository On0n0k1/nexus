#!/usr/bin/env bash
# tools/backfill-tags.sh — one-time backfill of git tags + GitHub Releases
# for already-published versions of every workspace crate.
#
# Reads CHANGELOG.md per crate, finds version sections with real content
# (skipping `## [Unreleased]` and `## [X.Y.Z] and earlier` stubs), locates
# the bump commit in git history, creates tag `<crate>-v<version>` at that
# commit, and creates a corresponding GitHub Release with the CHANGELOG
# section as notes.
#
# Usage:
#   tools/backfill-tags.sh --dry-run    # print proposed actions, don't execute
#   tools/backfill-tags.sh --execute    # do it for real
#
# Idempotent: tags / releases that already exist are skipped silently.

# NOTE: no `pipefail` — the bump_sha lookup intentionally early-exits awk,
# which SIGPIPEs the upstream `git log`. With pipefail that propagates as
# 141 and trips `set -e`. We don't otherwise depend on pipeline status.
set -eu

mode=""
case "${1:-}" in
    --dry-run) mode="dry" ;;
    --execute) mode="exec" ;;
    *) echo "Usage: $0 --dry-run | --execute" >&2; exit 1 ;;
esac

if [ ! -d ".git" ]; then
    echo "Error: run from workspace root" >&2
    exit 1
fi

# Collect existing tags so we can skip them.
existing_tags=$(git tag --list 'nexus-*-v*' | sort -u)
# Collect existing GH releases so we can skip them.
if [ "$mode" = "exec" ]; then
    existing_releases=$(gh release list --limit 1000 --json tagName --jq '.[].tagName' 2>/dev/null | sort -u || true)
else
    existing_releases=""
fi

actions=0
skipped_tag=0
skipped_release=0
warnings=0

for changelog in nexus-*/CHANGELOG.md; do
    crate=$(dirname "$changelog")

    # Find every dated `## [X.Y.Z] — DATE` section heading. Skip:
    #   ## [Unreleased]
    #   ## [X.Y.Z] and earlier      (stub, no real content)
    # Match `## [X.Y.Z] — YYYY-MM-DD` (with em dash) and similar separators.
    versions=$(awk '
        /^## \[[0-9]+\.[0-9]+\.[0-9]+\] [—-] / {
            match($0, /\[([0-9]+\.[0-9]+\.[0-9]+)\]/, arr)
            print arr[1]
        }
    ' "$changelog")

    if [ -z "$versions" ]; then
        continue
    fi

    while IFS= read -r version; do
        tag="${crate}-v${version}"

        # Find the bump commit: the first commit where Cargo.toml's
        # version line changed TO this version.
        # The awk `exit` after the first match guarantees one-line output;
        # no `head -1` (which would SIGPIPE the upstream `git log` and
        # trip pipefail).
        bump_sha=$(git log --reverse --format="%H" -p -- "$crate/Cargo.toml" \
                   | awk -v ver="$version" '
                        /^[a-f0-9]{40}$/ { sha = $0 }
                        $0 == "+version = \"" ver "\"" { print sha; exit }
                   ')

        if [ -z "$bump_sha" ]; then
            echo "WARN  $tag: no bump commit found in git history; skipping" >&2
            warnings=$((warnings + 1))
            continue
        fi

        if echo "$existing_tags" | grep -qx "$tag"; then
            skipped_tag=$((skipped_tag + 1))
            continue
        fi

        # Extract CHANGELOG section for this version.
        notes=$(awk -v ver="$version" '
            $0 ~ "^## \\[" ver "\\]" { p=1; print; next }
            p && $0 ~ "^## \\[" { exit }
            p { print }
        ' "$changelog")

        if [ -z "$notes" ]; then
            echo "WARN  $tag: CHANGELOG section empty; skipping" >&2
            warnings=$((warnings + 1))
            continue
        fi

        first_line=${notes%%$'\n'*}
        actions=$((actions + 1))

        if [ "$mode" = "dry" ]; then
            echo "TAG   $tag  @  $bump_sha"
            echo "        notes: $first_line"
        else
            # Tag → push → release. Per-tag push so gh release create can
            # find the tag on origin (otherwise it errors).
            git tag "$tag" "$bump_sha"
            git push origin "$tag" >/dev/null 2>&1
            echo "TAG   $tag  @  $bump_sha"

            if echo "$existing_releases" | grep -qx "$tag"; then
                skipped_release=$((skipped_release + 1))
            else
                tmp=$(mktemp)
                printf "%s\n" "$notes" > "$tmp"
                gh release create "$tag" \
                    --title "$crate v$version" \
                    --notes-file "$tmp" \
                    >/dev/null
                rm -f "$tmp"
                echo "RLSE  $tag"
            fi
        fi
    done <<< "$versions"
done

echo
echo "==> Summary"
echo "    actions planned: $actions"
echo "    tags skipped (already exist): $skipped_tag"
[ "$mode" = "exec" ] && echo "    releases skipped (already exist): $skipped_release"
echo "    warnings: $warnings"

if [ "$mode" = "dry" ]; then
    echo
    echo "Dry run complete. Re-run with --execute to apply."
    echo "After --execute, push tags with:"
    echo "    git push origin --tags"
fi
