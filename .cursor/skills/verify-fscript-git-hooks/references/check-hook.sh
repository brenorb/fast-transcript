#!/bin/sh
set -eu
root="$(mktemp -d)"
trap 'rm -rf "$root"' EXIT
mkdir "$root/scripts" "$root/.githooks" "$root/bin"
cp scripts/install-git-hooks.sh "$root/scripts/"
cp .githooks/pre-commit "$root/.githooks/"
git -C "$root" init -q
sh "$root/scripts/install-git-hooks.sh"
test "$(git -C "$root" config core.hooksPath)" = ".githooks"
cat > "$root/bin/cargo" <<'FAKE'
#!/bin/sh
test "$*" = 'fmt --check'
printf 'called\n' >> "$CALL_LOG"
exit "$FORMAT_STATUS"
FAKE
chmod +x "$root/bin/cargo"
export PATH="$root/bin:$PATH" CALL_LOG="$root/calls"
cd "$root"
printf 'notes\n' > notes.txt
git add notes.txt
FORMAT_STATUS=1 sh .githooks/pre-commit
test ! -e "$CALL_LOG"
printf 'fn main() {}\n' > example.rs
git add example.rs
if FORMAT_STATUS=1 sh .githooks/pre-commit; then
    echo 'Invalid Rust was accepted' >&2
    exit 1
fi
FORMAT_STATUS=0 sh .githooks/pre-commit
test "$(wc -l < "$CALL_LOG" | tr -d ' ')" = 2
