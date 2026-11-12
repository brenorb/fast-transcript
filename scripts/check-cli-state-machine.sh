#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
    echo "Usage: $0 <model.tla> [config.cfg]" >&2
    exit 2
fi

spec=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
config=${2:-${spec%.tla}.cfg}
config=$(cd "$(dirname "$config")" && pwd)/$(basename "$config")
jar=${TLA2TOOLS:?Set TLA2TOOLS to the absolute path of the TLC 1.7.4 JAR}
case "$jar" in
    /*) ;;
    *) echo "TLA2TOOLS must be an absolute path" >&2; exit 2 ;;
esac
digest=$(shasum -a 256 "$jar")
if [ "${digest%% *}" != 936a262061c914694dfd669a543be24573c45d5aa0ff20a8b96b23d01e050e88 ]; then
    echo "Expected the official TLA+ 1.7.4 release JAR" >&2
    exit 2
fi

scratch=$(mktemp -d "${TMPDIR:-/tmp}/fscript-tlc.XXXXXXXX")
trap 'rm -rf "$scratch"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
cp "$spec" "$scratch/"
cp "$config" "$scratch/model.cfg"
cd "$scratch"
"${JAVA:-java}" -XX:+UseParallelGC -cp "$jar" tlc2.TLC -workers 1 -fp 0 -config model.cfg "$(basename "$spec")"
