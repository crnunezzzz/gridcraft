#!/usr/bin/env bash
# Merge the original project into this fork (the IA build), settling by itself the conflicts that
# come back with every merge and leaving only real code conflicts for a person (or Claude).
#
# Usage: APP=wordcraft .github/ia/sync-upstream.sh <upstream-ref>
#        APP=wordcraft .github/ia/sync-upstream.sh --finish   (after resolving the conflicts)
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
#
# The merge base. The original rewrites its `release` branch at every release: «Release x.y.z» is
# a commit of its own on top of main, and the next release does not descend from it. A plain
# `git merge` then takes an old commit of main as the base, sees the last release's changes as
# ours, and conflicts on the version line at every release, and on every file the release commit
# touched. So the script records the upstream commit it merged in .github/ia/upstream.rev, and
# the next merge uses that commit as its base (through two temporary `git replace --graft` refs,
# removed before the script returns): only the lines this fork really changed can conflict.
set -euo pipefail

ref=${1:?usage: sync-upstream.sh <upstream-ref> | --finish}
: "${APP:?set APP (wordcraft, gridcraft, deckcraft)}"
rm -f conflicts.txt
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

# What follows every merge, whoever settled its conflicts.
after_merge() {
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
}

if [ "$ref" = "--finish" ]; then
    if [ -n "$(git diff --name-only --diff-filter=U)" ] || git grep -nE '^(<<<<<<<|>>>>>>>) ' -- . ':!*.md' > /dev/null; then
        echo "Conflicts are still unresolved."
        exit 2
    fi
    git add -A
    # Nothing to commit when the merge was clean and only our own crates needed a fix.
    if [ -f "$(git rev-parse --git-dir)/MERGE_HEAD" ]; then
        git commit --no-edit
    elif ! git diff --cached --quiet; then
        git commit -m "IA sync: our crates follow the original's changes"
    fi
    after_merge
    exit 0
fi

REV_FILE=.github/ia/upstream.rev
new=$(git rev-parse --verify "$ref^{commit}")

if git merge-base --is-ancestor "$new" HEAD; then
    echo "Already up to date with $ref."
    exit 0
fi

# Grafts never outlive the script: a stale one would change what every later git command sees.
drop_grafts() {
    git for-each-ref --format='%(refname)' refs/replace | while IFS= read -r r; do
        git update-ref -d "$r"
    done
}
drop_grafts
trap 'drop_grafts; rm -rf "$tmp"' EXIT

old=""
if [ -f "$REV_FILE" ]; then
    old=$(tr -d '[:space:]' < "$REV_FILE")
    # The commit need not be an ancestor: a pull request merged with «Squash and merge» keeps
    # the merged files but drops the original's commits from our history, and the recorded
    # commit is still the right base. It only has to be here; ask the original for it if not.
    if ! git cat-file -e "$old^{commit}" 2> /dev/null; then
        git fetch --no-tags upstream "$old" 2> /dev/null || true
    fi
    if ! git cat-file -e "$old^{commit}" 2> /dev/null; then
        echo "::warning::$REV_FILE names $old, which cannot be found: merging with git's own base."
        old=""
    fi
fi
if [ -n "$old" ] && [ "$old" != "$(git merge-base HEAD "$new")" ]; then
    echo "Merging $ref on the base of the last merged upstream commit, $old."
    git replace --graft HEAD "$old"
    git replace --graft "$new" "$old"
fi

# Written into the merge commit itself, so the record and the merge cannot drift apart.
record_rev() {
    printf '%s\n' "$new" > "$REV_FILE"
    git add -- "$REV_FILE"
}

# The last line of our section at the top of README.md.
FORK_END='<!-- fin de la sección del fork -->'

ours_only() { case "$1" in docs/android.md | docs/asistente-*.md | packaging/*/*-IA.txt | packaging/*/install-ia.*) return 0 ;; *) return 1 ;; esac; }

merged=0
git merge --no-commit --no-ff "$ref" || merged=$?
drop_grafts
if [ "$merged" -eq 0 ]; then
    record_rev
    git commit --no-edit
else
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
    record_rev
    remaining=$(git diff --name-only --diff-filter=U)
    if [ -n "$remaining" ]; then
        printf '%s\n' "$remaining" > conflicts.txt
        echo "Code conflicts remain:"
        cat conflicts.txt
        exit 2
    fi
    git commit --no-edit
fi


after_merge
