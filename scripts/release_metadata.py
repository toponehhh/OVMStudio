"""Validate the application version and export safe GitHub Actions outputs."""
import argparse
from pathlib import Path
import re
import tomllib

VERSION = re.compile(r'\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?')


def metadata(root: Path, tag: str | None = None) -> dict[str, str]:
    package = tomllib.loads((root / 'Cargo.toml').read_text(encoding='utf-8'))['package']
    version = package['version']
    if not VERSION.fullmatch(version):
        raise ValueError(f'Unsupported application version: {version}')
    locked = tomllib.loads((root / 'Cargo.lock').read_text(encoding='utf-8'))['package']
    own = [p for p in locked if p['name'] == package['name'] and 'source' not in p]
    if len(own) != 1 or own[0]['version'] != version:
        raise ValueError('Cargo.toml and Cargo.lock application versions must match')
    expected_tag = 'v' + version
    if tag is not None and tag != expected_tag:
        raise ValueError(f'Release tag {tag!r} must match Cargo.toml version {expected_tag!r}')
    return {'version': version, 'tag': expected_tag,
            'archive': f'OVMS-Studio-{version}-windows-x64.zip',
            'artifact': f'OVMS-Studio-{version}-windows-x64',
            'prerelease': str('-' in version.split('+')[0]).lower()}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parent.parent)
    parser.add_argument('--tag')
    parser.add_argument('--github-output', type=Path)
    args = parser.parse_args()
    result = metadata(args.root, args.tag)
    lines = ''.join(f'{key}={value}\n' for key, value in result.items())
    if args.github_output:
        with args.github_output.open('a', encoding='utf-8') as file:
            file.write(lines)
    print(lines, end='')
