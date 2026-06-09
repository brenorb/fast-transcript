#!/bin/sh
set -eu

repo_root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"

git -C "$repo_root" config core.hooksPath .githooks

echo "Configured git hooks path to $repo_root/.githooks"
