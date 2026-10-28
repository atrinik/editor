#!/usr/bin/env python3
"""Checks that a Windows executable loads no Visual C++ runtime DLL.

The C runtime is linked statically into release packages: a DLL such as
VCRUNTIME140.dll is not part of Windows, so where it is not installed the
loader searches the current directory for it before the editor can leave
that directory. This reads the executable's import and delay-load import
tables and fails when either names a Visual C++ runtime DLL.
"""
# Copyright (c) 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import re
import struct
import sys
from pathlib import Path

# Visual C++ runtime DLLs: shipped by the redistributable, not by Windows.
# The Universal CRT (ucrtbase.dll, api-ms-win-crt-*) is part of Windows 10
# and later, which the loader finds in the system folder first.
FORBIDDEN = re.compile(r'^(vcruntime|msvcp|vccorlib|concrt|vcomp|msvcr)[0-9_a-z]*\.dll$', re.IGNORECASE)
# Bounds on what a well-formed executable holds, so a damaged one cannot
# make the check run long.
MAXIMUM_DESCRIPTORS = 4096
MAXIMUM_NAME = 512
IMPORT_DIRECTORY = 1
DELAY_IMPORT_DIRECTORY = 13


class FormatError(Exception):
    pass


def unpack(fmt, data, offset):
    size = struct.calcsize(fmt)
    if offset < 0 or offset + size > len(data):
        raise FormatError(f'truncated at offset {offset}')
    return struct.unpack_from(fmt, data, offset)


class Image:
    def __init__(self, data):
        self.data = data
        if data[:2] != b'MZ':
            raise FormatError('not an MZ executable')
        (pe,) = unpack('<I', data, 0x3C)
        if data[pe:pe + 4] != b'PE\0\0':
            raise FormatError('no PE signature')
        _, sections, _, _, _, optional_size, _ = unpack('<HHIIIHH', data, pe + 4)
        optional = pe + 24
        (magic,) = unpack('<H', data, optional)
        if magic == 0x20B:
            (self.image_base,) = unpack('<Q', data, optional + 24)
            count_at, directories = optional + 108, optional + 112
        elif magic == 0x10B:
            (self.image_base,) = unpack('<I', data, optional + 28)
            count_at, directories = optional + 92, optional + 96
        else:
            raise FormatError(f'unknown optional header magic {magic:#x}')
        (count,) = unpack('<I', data, count_at)
        self.directories = [unpack('<II', data, directories + 8 * index) for index in range(min(count, 16))]
        table = optional + optional_size
        self.sections = []
        for index in range(sections):
            virtual_size, address, raw_size, raw = unpack('<IIII', data, table + 40 * index + 8)
            self.sections.append((address, max(virtual_size, raw_size), raw, raw_size))

    def offset(self, rva):
        for address, size, raw, raw_size in self.sections:
            if address <= rva < address + size:
                if rva - address >= raw_size:
                    raise FormatError(f'address {rva:#x} has no file data')
                return raw + rva - address
        raise FormatError(f'address {rva:#x} is in no section')

    def name(self, rva):
        start = self.offset(rva)
        end = self.data.find(b'\0', start, start + MAXIMUM_NAME)
        if end < 0:
            raise FormatError(f'unterminated name at {rva:#x}')
        return self.data[start:end].decode('ascii', 'replace')

    def directory(self, index):
        return self.directories[index] if index < len(self.directories) else (0, 0)

    def imports(self):
        """Every DLL named by the import and delay-load import tables."""
        names = []
        rva, size = self.directory(IMPORT_DIRECTORY)
        if rva and size:
            at = self.offset(rva)
            for index in range(MAXIMUM_DESCRIPTORS):
                descriptor = unpack('<IIIII', self.data, at + 20 * index)
                if not any(descriptor):
                    break
                names.append(self.name(descriptor[3]))
            else:
                raise FormatError('too many import descriptors')
        rva, size = self.directory(DELAY_IMPORT_DIRECTORY)
        if rva and size:
            at = self.offset(rva)
            for index in range(MAXIMUM_DESCRIPTORS):
                descriptor = unpack('<8I', self.data, at + 32 * index)
                if not any(descriptor):
                    break
                attributes, name = descriptor[0], descriptor[1]
                # Old-style descriptors hold 32-bit addresses, not relative
                # ones.
                names.append(self.name(name if attributes & 1 else (name - self.image_base) & 0xFFFFFFFF))
            else:
                raise FormatError('too many delay-load import descriptors')
        return names


def forbidden(names):
    return sorted({name for name in names if FORBIDDEN.match(name)}, key=str.lower)


def main(arguments):
    if len(arguments) != 1:
        print('usage: check-windows-imports.py EXECUTABLE', file=sys.stderr)
        return 2
    try:
        names = Image(Path(arguments[0]).read_bytes()).imports()
    except (OSError, FormatError) as error:
        print(f'cannot read the import table of {arguments[0]}: {error}', file=sys.stderr)
        return 1
    found = forbidden(names)
    if found:
        print(f'{arguments[0]} loads Visual C++ runtime DLLs; link the C runtime statically: {", ".join(found)}', file=sys.stderr)
        return 1
    print(f'{arguments[0]} imports {len(names)} DLLs, none of them a Visual C++ runtime DLL')
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
