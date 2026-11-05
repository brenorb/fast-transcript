#!/usr/bin/env python3
"""Generate the checked-in man page from the offline Markdown manual."""
import argparse
from pathlib import Path
import re
import sys
import textwrap

ROOT = Path(__file__).resolve().parents[1]


def escape(text):
    text = text.replace('\\', r'\e').replace('-', r'\-')
    return r'\&' + text if text.startswith(('.', "'")) else text


def inline(text):
    return re.sub(r'`([^`]+)`', lambda match: r'\fB' + match[1] + r'\fR', escape(text))


def render(source):
    version = re.search(r'^version = "([^"]+)"', (ROOT / 'Cargo.toml').read_text(), re.M)[1]
    lines = [f'.TH FSCRIPT 1 "October 22, 2026" "fast-transcript {version}" "User Commands"']
    code = False
    paragraph = True
    for line in source.splitlines():
        if line.startswith('```'):
            lines.append('.fi' if code else '.nf')
            code = not code
            paragraph = True
        elif code:
            lines.append(escape(line))
        elif line.startswith('# '):
            continue
        elif line.startswith('## '):
            heading = line[3:].upper()
            lines.append('.SH ' + (f'"{heading}"' if ' ' in heading else heading))
            paragraph = False
        elif line.startswith('- '):
            lines.append(r'.IP \(bu 2')
            lines.extend(textwrap.wrap(inline(line[2:]), width=78, break_long_words=False, break_on_hyphens=False))
            paragraph = True
        elif line.strip():
            if paragraph and not lines[-1].startswith('.SH'):
                lines.append('.PP')
            lines.extend(textwrap.wrap(inline(line), width=78, break_long_words=False, break_on_hyphens=False))
            paragraph = False
        else:
            paragraph = True
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='fail if the generated page is stale')
    args = parser.parse_args()
    output = ROOT / 'docs/fscript.1'
    generated = render((ROOT / 'docs/cli.md').read_text())
    if args.check:
        if not output.exists() or output.read_text() != generated:
            print('Man page is stale; run python3 scripts/generate_man_page.py', file=sys.stderr)
            return 1
    else:
        output.write_text(generated)
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
