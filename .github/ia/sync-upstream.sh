#!/usr/bin/env bash
# Merge the original project into this fork (the IA build), settling by itself the conflicts that
# come back with every merge and leaving only real code conflicts for a person (or Claude).
#
# Usage: APP=wordcraft .github/ia/sync-upstream.sh <upstream-ref>
# Exit:  0 merged (or already up to date) and committed · 2 code conflicts remain (listed in
#        conflicts.txt, the merge is left in progress) · anything else: an error.
#
# What it settles:
# - Cargo.lock: the original's, then refreshed from the merged manifests (`cargo metadata`).
# - The original's progress docs (ROADMAP.md, docs/*.md, …): the original's text. This fork's
#   own notes live in files the original never has (docs/android.md, docs/asistente-*.md).
# - ATTRIBUTION.md: the original's table, plus every row of ours it lacks (our Android icon).
# - README.md: our section (down to the «fin de la sección del fork» marker) on top of the original's.
# - docs/brand/: the ArtCraft logos stay removed, in the README too: their licence
#   (docs/brand/LICENSE-brand.txt) makes a published modified version remove them.
# Then it keeps the assistant's craft-ui on the same craft-libs rev as crates/ui-egui.
set -euo pipefail

ref=${1:?usage: sync-upstream.sh <upstream-ref>}
: "${APP:?set APP (wordcraft, gridcraft, deckcraft)}"
rm -f conflicts.txt
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

if git merge-base --is-ancestor "$ref" HEAD; then
    echo "Already up to date with $ref."
    exit 0
fi

# The last line of our section at the top of README.md.
FORK_END='<!-- fin de la sección del fork -->'

ours_only() { case "$1" in docs/android.md | docs/asistente-*.md | packaging/*/*-IA.txt | packaging/*/install-ia.*) return 0 ;; *) return 1 ;; esac; }

if ! git merge --no-edit --no-ff "$ref"; then
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        case "$f" in
            Cargo.lock)
                git checkout --theirs -- "$f" ;;
            ATTRIBUTION.md)
                git show ":2:$f" > "$tmp/ours.md" 2> /dev/null || : > "$tmp/ours.md"
                git checkout --theirs -- "$f"
                grep '^| ' "$tmp/ours.md" | while IFS= read -r row; do
                    grep -qxF -- "$row" "$f" || printf '%s\n' "$row" >> "$f"
                done ;;
            README.md)
                git show ":2:$f" > "$tmp/ours.md" 2> /dev/null || : > "$tmp/ours.md"
                git checkout --theirs -- "$f"
                if grep -qF -- "$FORK_END" "$tmp/ours.md"; then
                    { sed "/$FORK_END/q" "$tmp/ours.md"; printf '\n'; cat "$f"; } > "$tmp/readme" && cat "$tmp/readme" > "$f"
                else
                    banner=$(sed '/^#/,$d' "$tmp/ours.md")
                    if [ -n "$banner" ] && ! grep -qF -- "$(printf '%s\n' "$banner" | head -n1)" "$f"; then
                        { printf '%s\n\n' "$banner"; cat "$f"; } > "$tmp/readme" && cat "$tmp/readme" > "$f"
                    fi
                fi ;;
            docs/brand/*.svg | docs/brand/*.png)
                git rm -q --ignore-unmatch -- "$f"
                continue ;;
            *.md)
                if ours_only "$f"; then git checkout --ours -- "$f"; else git checkout --theirs -- "$f"; fi ;;
            *) continue ;;
        esac
        git add -- "$f"
    done < <(git diff --name-only --diff-filter=U)
    remaining=$(git diff --name-only --diff-filter=U)
    if [ -n "$remaining" ]; then
        printf '%s\n' "$remaining" > conflicts.txt
        echo "Code conflicts remain:"
        cat conflicts.txt
        exit 2
    fi
    git commit --no-edit
fi

# The ArtCraft logos stay out of this modified version, whatever the merge brought back.
git rm -q --ignore-unmatch -- docs/brand/*.svg docs/brand/*.png 2> /dev/null || true
perl -0pi -e 's{<p align="center">\s*<a href="https://getartcraft\.com/">.*?</a>.*?</p>\n\n?}{}gs' README.md
git add -A README.md docs/brand 2> /dev/null || true
if ! git diff --cached --quiet; then
    git commit -m "IA sync: the ArtCraft logos stay out of the modified version"
fi

# The assistant's combo box comes from craft-ui: keep it on the editor's craft-libs rev, so one
# copy is built and its look follows the editor's.
rev=$(sed -n 's/^craft-ui = { git = "[^"]*", rev = "\([0-9a-f]\{40\}\)".*/\1/p' crates/ui-egui/Cargo.toml | head -n1)
manifest="apps/$APP-assistant/Cargo.toml"
if [ -n "$rev" ] && [ -f "$manifest" ]; then
    sed -i "s/^\(craft-ui = { git = \"[^\"]*\", rev = \"\)[0-9a-f]\{40\}\"/\1$rev\"/" "$manifest"
fi

# The lock file follows the merged manifests (new crates of ours, versions of theirs).
cargo metadata --format-version 1 > /dev/null
git add -A Cargo.lock "$manifest" 2> /dev/null || true
if ! git diff --cached --quiet; then
    git commit -m "IA sync: lock file and assistant dependencies follow the merged manifests"
fi
