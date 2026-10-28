"""Proof that the Windows import check reads both import tables."""
# Copyright (c) 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import importlib.util
from pathlib import Path
import struct
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('check_windows_imports', Path(__file__).with_name('check-windows-imports.py'))
check = importlib.util.module_from_spec(spec)
spec.loader.exec_module(check)

SECTION_RVA = 0x1000
SECTION_FILE = 0x200
IMAGE_BASE = 0x140000000


def executable(imports, delayed=(), old_style=False):
    """A minimal PE32+ image whose one section holds both import tables."""
    names = bytearray()
    offsets = {}
    descriptors = 20 * (len(imports) + 1)
    delay = 32 * (len(delayed) + 1)
    for name in [*imports, *delayed]:
        offsets[name] = SECTION_RVA + descriptors + delay + len(names)
        names += name.encode() + b'\0'
    section = bytearray()
    for name in imports:
        section += struct.pack('<IIIII', 0, 0, 0, offsets[name], 0)
    section += bytes(20)
    for name in delayed:
        attributes, address = (0, IMAGE_BASE + offsets[name]) if old_style else (1, offsets[name])
        section += struct.pack('<8I', attributes, address & 0xFFFFFFFF, 0, 0, 0, 0, 0, 0)
    section += bytes(32)
    section += names
    optional = bytearray(240)
    struct.pack_into('<H', optional, 0, 0x20B)
    struct.pack_into('<Q', optional, 24, IMAGE_BASE)
    struct.pack_into('<I', optional, 108, 16)
    struct.pack_into('<II', optional, 112 + 8 * 1, SECTION_RVA, descriptors)
    if delayed:
        struct.pack_into('<II', optional, 112 + 8 * 13, SECTION_RVA + descriptors, delay)
    header = bytearray(0x40)
    header[:2] = b'MZ'
    struct.pack_into('<I', header, 0x3C, 0x40)
    header += b'PE\0\0' + struct.pack('<HHIIIHH', 0x8664, 1, 0, 0, 0, len(optional), 0x22)
    header += optional
    header += b'.idata\0\0' + struct.pack('<IIII', len(section), SECTION_RVA, len(section), SECTION_FILE) + bytes(16)
    return bytes(header.ljust(SECTION_FILE, b'\0') + section)


class CheckWindowsImportsTests(unittest.TestCase):
    def run_check(self, data):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'tilewright-editor.exe'
            path.write_bytes(data)
            return check.main([str(path)])

    def test_reads_both_tables(self):
        image = check.Image(executable(['KERNEL32.dll', 'api-ms-win-crt-runtime-l1-1-0.dll'], ['bcrypt.dll']))
        self.assertEqual(image.imports(), ['KERNEL32.dll', 'api-ms-win-crt-runtime-l1-1-0.dll', 'bcrypt.dll'])
        old = check.Image(executable(['KERNEL32.dll'], ['USER32.dll'], old_style=True))
        self.assertEqual(old.imports(), ['KERNEL32.dll', 'USER32.dll'])

    def test_system_dlls_pass(self):
        self.assertEqual(self.run_check(executable(['KERNEL32.dll', 'ntdll.dll', 'ucrtbase.dll'], ['d3d12.dll'])), 0)

    def test_visual_cpp_runtime_dlls_fail(self):
        self.assertEqual(self.run_check(executable(['KERNEL32.dll', 'VCRUNTIME140.dll'])), 1)
        self.assertEqual(self.run_check(executable(['KERNEL32.dll'], ['msvcp140.dll'])), 1)
        self.assertEqual(self.run_check(executable(['vcruntime140_1.dll'])), 1)
        self.assertEqual(check.forbidden(['VCRUNTIME140.dll', 'kernel32.dll', 'MSVCP140_ATOMIC_WAIT.dll']),
                         ['MSVCP140_ATOMIC_WAIT.dll', 'VCRUNTIME140.dll'])

    def test_damaged_executables_fail(self):
        data = executable(['KERNEL32.dll'])
        self.assertEqual(self.run_check(b'not an executable'), 1)
        self.assertEqual(self.run_check(data[:0x150]), 1)
        # A name that runs off the end of the section.
        self.assertEqual(self.run_check(data.rstrip(b'\0') + b'x' * 600), 1)


if __name__ == '__main__':
    unittest.main()
