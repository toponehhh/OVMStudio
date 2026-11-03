"""Publish a verified bundle with the ephemeral GITHUB_TOKEN supplied by Actions."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
from urllib.parse import quote
import zipfile

from release_metadata import VERSION


def run_gh(arguments: list[str]) -> str:
    result = subprocess.run(['gh', *arguments], capture_output=True, text=True,
                            encoding='utf-8', errors='replace', timeout=240)
    if result.returncode:
        raise RuntimeError(result.stderr.strip() or 'GitHub CLI failed')
    return result.stdout


def api(path: str, fields: dict[str, str] | None = None, optional: bool = False):
    arguments = ['api', path]
    if fields is not None:
        arguments.extend(['--method', 'POST'])
        for key, value in fields.items():
            arguments.extend(['--raw-field', f'{key}={value}'])
    try:
        return json.loads(run_gh(arguments))
    except RuntimeError as error:
        if optional and 'HTTP 404' in str(error):
            return None
        raise


def release_info(repository: str, tag: str, optional: bool = False):
    # REST's /releases/tags/{tag} resolves published releases only. The CLI
    # also resolves pending draft tags through GraphQL; use that database ID
    # for the REST response so a retry can find its own unpublished draft.
    try:
        result = json.loads(run_gh(['release', 'view', tag, '--repo', repository,
                                   '--json', 'databaseId']))
    except RuntimeError as error:
        if optional and 'release not found' in str(error).lower():
            return None
        raise
    release_id = result['databaseId']
    if type(release_id) is not int or release_id <= 0:
        raise ValueError('GitHub returned an invalid release ID')
    return api(f'repos/{repository}/releases/{release_id}')


def validate_assets(archive: Path, checksum: Path, version: str) -> dict:
    if not VERSION.fullmatch(version):
        raise ValueError('Invalid application version')
    expected = f'OVMS-Studio-{version}-windows-x64.zip'
    if archive.name != expected or checksum.name != expected + '.sha256':
        raise ValueError('Release asset filenames must match the application version')
    fields = checksum.read_text(encoding='ascii').strip().split(maxsplit=1)
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    if len(fields) != 2 or fields[0].lower() != digest or fields[1].lstrip('*') != archive.name:
        raise ValueError('Release archive SHA-256 verification failed')
    with zipfile.ZipFile(archive) as source:
        manifest = json.loads(source.read('OVMS-Studio/bundle-manifest.json').decode('utf-8-sig'))
        if (manifest['application_version'] != version or manifest.get('distribution') != 'ui-only'
                or manifest.get('runtime_delivery') != 'on-demand'
                or set(manifest.get('supported_backends', [])) != {'native', 'docker'}):
            raise ValueError('Bundle manifest does not match the release')
        if hashlib.sha256(source.read('OVMS-Studio/OVMS-Studio.exe')).hexdigest() != manifest['application_sha256']:
            raise ValueError('Application executable SHA-256 mismatch')
        if any(name.lower().startswith('ovms-studio/ovms/') or name.lower().endswith(('/ovms.exe', '/python.exe'))
               for name in source.namelist()):
            raise ValueError('UI-only releases must not contain an OVMS runtime payload')
    return manifest


def tag_commit(repository: str, tag: str) -> str | None:
    ref = api(f'repos/{repository}/git/ref/tags/{quote(tag, safe="")}', optional=True)
    if ref is None:
        return None
    obj = ref['object']
    for _ in range(8):
        if obj['type'] == 'commit':
            return obj['sha']
        if obj['type'] != 'tag':
            raise ValueError('Release tag does not reference a commit')
        obj = api(f'repos/{repository}/git/tags/{obj["sha"]}')['object']
    raise ValueError('Release tag has too many nested annotated tags')


def publish(repository: str, commit: str, version: str, archive: Path, checksum: Path):
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('Invalid GitHub repository')
    if not re.fullmatch(r'[0-9a-fA-F]{40}', commit):
        raise ValueError('A full source commit SHA is required')
    manifest = validate_assets(archive, checksum, version)
    tag = 'v' + version
    release = release_info(repository, tag, optional=True)
    if release and not release['draft']:
        names = {a['name'] for a in release['assets']}
        if not {archive.name, checksum.name}.issubset(names):
            raise ValueError('Published release is incomplete; existing releases are never overwritten')
        print(f'Already published; leaving release unchanged: {release["html_url"]}')
        return release['html_url']

    marker = f'<!-- OVMStudio automated release; version={version}; commit={commit} -->'
    if release and marker not in (release.get('body') or ''):
        raise ValueError('Existing draft is not owned by this workflow and source commit')
    resolved = tag_commit(repository, tag)
    if resolved is None:
        api(f'repos/{repository}/git/refs', {'ref': f'refs/tags/{tag}', 'sha': commit})
    elif resolved.lower() != commit.lower():
        raise ValueError('Existing version tag points to another commit; bump the application version')

    if release is None:
        notes = f'''{marker}

## Windows x64 UI 程序包

- 原生 Rust + Slint UI，程序包不包含 OVMS 运行包或 Docker 镜像。
- 解压后运行 `OVMS-Studio/OVMS-Studio.exe`，在“设置”中选择本机或 Docker。
- 本机模式自动下载最新官方 OVMS；Docker 模式检查本机引擎后按目标设备拉取最新官方镜像。
- 后续启动复用缓存，在设置中可检查并更新最新版本。
- 构建通过契约测试、本机真实推理、按需下载及 UI 验证；发布前还要求真实 Docker 集成测试通过。

源代码：[本次提交](https://github.com/{repository}/commit/{commit})。
'''
        with tempfile.TemporaryDirectory() as directory:
            notes_file = Path(directory) / 'release-notes.md'
            notes_file.write_text(notes, encoding='utf-8')
            arguments = ['release', 'create', tag, '--repo', repository, '--target', commit,
                         '--verify-tag', '--draft', '--title', f'OVMS Studio {tag}',
                         '--generate-notes', '--notes-file', str(notes_file)]
            if '-' in version.split('+')[0]:
                arguments.append('--prerelease')
            run_gh(arguments)
    # Overwriting assets is limited to the workflow-owned unpublished draft.
    run_gh(['release', 'upload', tag, str(archive), str(checksum), '--repo', repository, '--clobber'])
    draft = release_info(repository, tag)
    if not {archive.name, checksum.name}.issubset({a['name'] for a in draft['assets']}):
        raise ValueError('Both release assets must be uploaded before publishing')
    run_gh(['release', 'edit', tag, '--repo', repository, '--draft=false'])
    published = release_info(repository, tag)
    if published['draft']:
        raise ValueError('GitHub did not publish the draft release')
    print(f'Published: {published["html_url"]}')
    return published['html_url']


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repo', default=os.environ.get('GITHUB_REPOSITORY'))
    parser.add_argument('--commit', default=os.environ.get('GITHUB_SHA'))
    parser.add_argument('--version', required=True)
    parser.add_argument('--archive', required=True, type=Path)
    parser.add_argument('--checksum', required=True, type=Path)
    args = parser.parse_args()
    publish(args.repo, args.commit, args.version, args.archive, args.checksum)
