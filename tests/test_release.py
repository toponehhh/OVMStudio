"""Release safety contracts. All GitHub operations are mocked."""
import hashlib
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile

BASE = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(BASE / 'scripts'))
import publish_release
import release_metadata


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        base = BASE / 'work/release-unit-tests'
        base.mkdir(parents=True, exist_ok=True)
        self.directory = tempfile.TemporaryDirectory(dir=base)
        self.root = Path(self.directory.name).resolve()
        self.assertTrue(self.root.is_relative_to(base.resolve()))
        self.version = '0.1.1'
        self.commit = 'a' * 40
        self.archive = self.root / f'OVMS-Studio-{self.version}-windows-x64.zip'
        self.checksum = Path(str(self.archive) + '.sha256')
        gui, runtime = b'test GUI', b'test runtime'
        manifest = {'application_version': self.version, 'runtime_version': '2026.4.1',
                    'runtime_flavor': 'python_on',
                    'application_sha256': hashlib.sha256(gui).hexdigest(),
                    'runtime_executable_sha256': hashlib.sha256(runtime).hexdigest()}
        with zipfile.ZipFile(self.archive, 'w') as archive:
            archive.writestr('OVMS-Studio/bundle-manifest.json', json.dumps(manifest))
            archive.writestr('OVMS-Studio/OVMS-Studio.exe', gui)
            archive.writestr('OVMS-Studio/ovms/ovms.exe', runtime)
            archive.writestr('OVMS-Studio/ovms/python/python.exe', b'test Python')
            archive.writestr('OVMS-Studio/ovms/LICENSE', b'test license')
        self.checksum.write_text(hashlib.sha256(self.archive.read_bytes()).hexdigest() +
                                 '  ' + self.archive.name + '\n', encoding='ascii')

    def tearDown(self):
        self.directory.cleanup()

    def cargo(self, version='0.1.1', locked='0.1.1'):
        (self.root / 'Cargo.toml').write_text(f'[package]\nname="app"\nversion="{version}"\n', encoding='utf-8')
        (self.root / 'Cargo.lock').write_text(f'version=4\n[[package]]\nname="app"\nversion="{locked}"\n', encoding='utf-8')

    def test_tag_must_match_the_locked_application_version(self):
        self.cargo()
        with self.assertRaises(ValueError):
            release_metadata.metadata(self.root, 'v99.0.0')
        self.cargo(locked='0.1.0')
        with self.assertRaises(ValueError):
            release_metadata.metadata(self.root)

    def test_prerelease_is_preserved_in_tag_and_asset_names(self):
        self.cargo('1.2.3-beta.1', '1.2.3-beta.1')
        result = release_metadata.metadata(self.root, 'v1.2.3-beta.1')
        self.assertEqual(result['prerelease'], 'true')
        self.assertEqual(result['archive'], 'OVMS-Studio-1.2.3-beta.1-windows-x64.zip')

    def test_corrupt_assets_never_reach_github(self):
        self.archive.write_bytes(self.archive.read_bytes() + b'tampered')
        with patch.object(publish_release, 'api') as github:
            with self.assertRaises(ValueError):
                publish_release.publish('toponehhh/OVMStudio', self.commit, self.version, self.archive, self.checksum)
            github.assert_not_called()

    def test_published_version_is_not_replaced_by_later_main_commits(self):
        release = {'draft': False, 'html_url': 'https://example.test/release',
                   'assets': [{'name': self.archive.name}, {'name': self.checksum.name}]}
        with patch.object(publish_release, 'api', return_value=release) as github, patch.object(publish_release, 'run_gh') as command:
            result = publish_release.publish('toponehhh/OVMStudio', self.commit, self.version, self.archive, self.checksum)
            self.assertEqual(result, release['html_url'])
            self.assertEqual(github.call_count, 1)
            command.assert_not_called()

    def test_existing_tag_cannot_be_moved_to_another_commit(self):
        with patch.object(publish_release, 'api', side_effect=[None, {'object': {'type': 'commit', 'sha': 'b' * 40}}]) as github, patch.object(publish_release, 'run_gh') as command:
            with self.assertRaisesRegex(ValueError, 'another commit'):
                publish_release.publish('toponehhh/OVMStudio', self.commit, self.version, self.archive, self.checksum)
            self.assertTrue(all(call.kwargs.get('fields') is None for call in github.call_args_list))
            command.assert_not_called()

    def test_manual_draft_is_not_overwritten(self):
        with patch.object(publish_release, 'api', return_value={'draft': True, 'body': 'manual release'}) as github, patch.object(publish_release, 'run_gh') as command:
            with self.assertRaisesRegex(ValueError, 'not owned'):
                publish_release.publish('toponehhh/OVMStudio', self.commit, self.version, self.archive, self.checksum)
            self.assertEqual(github.call_count, 1)
            command.assert_not_called()

    def test_incomplete_published_release_does_not_get_replaced(self):
        with patch.object(publish_release, 'api', return_value={'draft': False, 'assets': []}), patch.object(publish_release, 'run_gh') as command:
            with self.assertRaisesRegex(ValueError, 'incomplete'):
                publish_release.publish('toponehhh/OVMStudio', self.commit, self.version, self.archive, self.checksum)
            command.assert_not_called()

    def test_both_assets_are_uploaded_before_the_draft_becomes_public(self):
        state = {'release': None, 'tag': None}
        operations = []

        def github(path, fields=None, optional=False):
            if path.endswith('/releases/tags/v0.1.1'):
                return state['release']
            if '/git/ref/tags/' in path:
                return state['tag']
            if path.endswith('/git/refs'):
                self.assertEqual(fields['sha'], self.commit)
                state['tag'] = {'object': {'type': 'commit', 'sha': self.commit}}
                operations.append('tag')
                return state['tag']
            raise AssertionError(path)

        def command(arguments):
            operation = arguments[1]
            operations.append(operation)
            if operation == 'create':
                notes = Path(arguments[arguments.index('--notes-file') + 1]).read_text(encoding='utf-8')
                self.assertIn(f'commit={self.commit}', notes)
                self.assertIn('--draft', arguments)
                state['release'] = {'draft': True, 'body': notes, 'assets': [], 'html_url': 'https://example.test/release'}
            elif operation == 'upload':
                self.assertTrue(state['release']['draft'])
                state['release']['assets'] = [{'name': self.archive.name}, {'name': self.checksum.name}]
            elif operation == 'edit':
                self.assertEqual(len(state['release']['assets']), 2)
                state['release']['draft'] = False
            else:
                raise AssertionError(arguments)
            return ''

        with patch.object(publish_release, 'api', side_effect=github), patch.object(publish_release, 'run_gh', side_effect=command):
            publish_release.publish('toponehhh/OVMStudio', self.commit, self.version, self.archive, self.checksum)
        self.assertEqual(operations, ['tag', 'create', 'upload', 'edit'])


if __name__ == '__main__':
    unittest.main()
