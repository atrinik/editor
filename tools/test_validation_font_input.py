"""Prove aggregate/Windows tests receive the explicitly prepared font root."""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class ValidationFontInputTests(unittest.TestCase):
    def fixture(self, root):
        (root / 'tools').mkdir()
        shutil.copyfile(ROOT / 'tools/validate.sh', root / 'tools/validate.sh')
        subprocess.run(['git', 'init', '--quiet', str(root)], check=True)
        binary = root / 'bin'
        binary.mkdir()
        for command in ('python3', 'cargo-deny', 'cargo-auditable', 'jq', 'syft'):
            path = binary / command
            path.write_text('#!/bin/sh\nexit 0\n')
            path.chmod(0o755)
        rustc = binary / 'rustc'
        rustc.write_text('#!/bin/sh\nprintf "rustc 1.97.1 (test)\\n"\n')
        rustc.chmod(0o755)
        cargo = binary / 'cargo'
        cargo.write_text(f'''#!{sys.executable}
import json, os, pathlib, sys
with pathlib.Path(os.environ['FONT_INPUT_COMMAND_LOG']).open('a') as output:
    output.write(json.dumps({{'args': sys.argv[1:], 'font_root': os.environ.get('TILEWRIGHT_UNICODE_FONT_ROOT')}}) + '\\n')
sys.exit(37 if sys.argv[1] == 'test' else 0)
''')
        cargo.chmod(0o755)
        environment = os.environ.copy()
        environment.update(PATH=str(binary) + os.pathsep + environment['PATH'],
                           FONT_INPUT_COMMAND_LOG=str(root / 'commands.jsonl'),
                           TILEWRIGHT_UNICODE_FONT_ROOT='stale ambient test input')
        environment.pop('TILEWRIGHT_PREPARED_FONT_PACK', None)
        return environment

    def test_required_pack_refused_before_quality_commands(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            environment = self.fixture(root)
            result = subprocess.run(['bash', 'tools/validate.sh'], cwd=root, env=environment, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn(b'TILEWRIGHT_PREPARED_FONT_PACK', result.stderr)
            self.assertFalse((root / 'commands.jsonl').exists())

    def test_explicit_pack_replaces_ambient_input_before_cargo_tests(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            environment = self.fixture(root)
            expected = str(root / 'prepared font pack')
            environment['TILEWRIGHT_PREPARED_FONT_PACK'] = expected
            result = subprocess.run(['bash', 'tools/validate.sh'], cwd=root, env=environment, capture_output=True)
            self.assertEqual(result.returncode, 37, result.stderr.decode())
            commands = [json.loads(line) for line in (root / 'commands.jsonl').read_text().splitlines()]
            self.assertEqual(commands[-1]['args'], ['test', '--locked', '--workspace', '--all-targets'])
            self.assertTrue(all(command['font_root'] == expected for command in commands))

    def test_windows_test_step_has_prepared_font_environment(self):
        workflow = (ROOT / '.github/workflows/validate.yml').read_text()
        windows = workflow.split('\n  windows:\n', 1)[1].split('\n  aggregate:\n', 1)[0]
        self.assertLess(windows.index('prepare-font-pack.py prepare --acquire'), windows.index('cargo test --locked'))
        self.assertRegex(windows, re.compile(
            r'      - run: cargo test --locked --workspace --all-targets\n'
            r'        env:\n'
            r'          TILEWRIGHT_UNICODE_FONT_ROOT: \$\{\{ runner.temp \}\}/unicode-font-pack\n'))


if __name__ == '__main__':
    unittest.main()
