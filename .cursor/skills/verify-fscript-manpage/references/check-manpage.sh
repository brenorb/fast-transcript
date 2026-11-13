#!/bin/sh
set -eu
temporary="$(mktemp -d)"
trap 'rm -rf "$temporary"' EXIT HUP INT TERM
mkdir -p "$temporary/scripts" "$temporary/docs"
cp scripts/generate_man_page.py "$temporary/scripts/"
cp Cargo.toml "$temporary/"
cp docs/cli.md "$temporary/docs/"
page="$temporary/docs/fscript.1"
printf 'stale fixture\n' > "$page"
if python3 "$temporary/scripts/generate_man_page.py" --check; then
    echo "Stale man page accepted" >&2
    exit 1
fi
test "$(cat "$page")" = 'stale fixture'
python3 "$temporary/scripts/generate_man_page.py"
python3 "$temporary/scripts/generate_man_page.py" --check
