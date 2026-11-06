#!/usr/bin/env python3
"""Provision a fresh Linux-only, private prototype session; never adopt state.

This trusted deployment helper establishes World/recovery separation that the
released Rust SourceRoot API cannot independently prove. It copies data only;
project data never chooses executable commands. A retained descriptor is passed
to the app, avoiding a pathname reopen after verification.
"""
# Copyright 2026 Seraphina Studios. SPDX-License-Identifier: MIT
import argparse
import json
import os
from pathlib import Path
import stat
import subprocess
import sys

MAX_FILES = 4096
MAX_BYTES = 64 << 20
PERSISTENT_LOCAL = {'ext2', 'ext3', 'ext4', 'xfs', 'btrfs', 'f2fs', 'zfs', 'bcachefs'}


def directory(path):
    return os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)


def kernel_text(path, maximum=1 << 20):
    with open(path, 'rb') as stream:
        data = stream.read(maximum + 1)
    if len(data) > maximum:
        raise ValueError('kernel mount evidence exceeds bound')
    return data.decode('ascii')


def persistent_mount(fd):
    """Correlate this retained directory FD with current kernel mount evidence.

    World also checks ACL/filesystem compatibility. This additional deployment
    check excludes volatile filesystems whose fsync succeeds without surviving
    reboot. It does not attest physical hardware or hostile root processes.
    """
    value = os.fstat(fd)
    ids = [line.split()[1] for line in kernel_text(f'/proc/self/fdinfo/{fd}', 4096).splitlines()
           if line.startswith('mnt_id:')]
    if len(ids) != 1 or not ids[0].isdigit():
        raise ValueError('cannot identify retained directory mount')
    matches = [line for line in kernel_text('/proc/self/mountinfo').splitlines()
               if line.split(' ', 1)[0] == ids[0]]
    if len(matches) != 1 or matches[0].count(' - ') != 1:
        raise ValueError('retained directory mount is unknown')
    before, after = matches[0].split(' - ')
    fields, filesystem = before.split(), after.split()
    device = f'{os.major(value.st_dev)}:{os.minor(value.st_dev)}'
    if len(fields) < 6 or len(filesystem) < 3 or fields[2] != device:
        raise ValueError('retained mount identity does not match directory device')
    kind = filesystem[0]
    if kind not in PERSISTENT_LOCAL or 'rw' not in fields[5].split(','):
        raise ValueError(f'prototype durable editing refuses volatile, unknown or unsupported filesystem: {kind}')
    return {'mount_id': int(ids[0]), 'device': device, 'filesystem': kind}


def identity(fd):
    value = os.fstat(fd)
    if not stat.S_ISDIR(value.st_mode) or value.st_uid != os.getuid() or stat.S_IMODE(value.st_mode) != 0o700:
        raise ValueError('session directories must be owned by this user with mode 0700')
    return {'device': value.st_dev, 'inode': value.st_ino, 'uid': value.st_uid, 'mode': '0700',
            'mount': persistent_mount(fd)}


def copy_tree(source, destination):
    """Copy bounded regular data without links or special files, then sync it."""
    files = 0
    total = 0
    entries = 0
    # Source is explicitly supplied data, not executable input. No ignored files
    # or ambient contents are bundled into releases by this provisioning helper.
    def visit(src_fd, dst_fd, depth=0):
        nonlocal files, total, entries
        if depth > 32:
            raise ValueError('sample exceeds directory depth bound')
        for name in sorted(os.listdir(src_fd)):
            entries += 1
            if entries > MAX_FILES:
                raise ValueError('sample exceeds entry bound')
            if name in ('.', '..') or '/' in name:
                raise ValueError('invalid source entry')
            value = os.stat(name, dir_fd=src_fd, follow_symlinks=False)
            if stat.S_ISDIR(value.st_mode):
                os.mkdir(name, mode=0o700, dir_fd=dst_fd)
                child_src = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=src_fd)
                child_dst = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=dst_fd)
                try:
                    visit(child_src, child_dst, depth + 1)
                finally:
                    os.close(child_src)
                    os.close(child_dst)
            elif stat.S_ISREG(value.st_mode):
                files += 1
                total += value.st_size
                if files > MAX_FILES or total > MAX_BYTES:
                    raise ValueError('sample exceeds provisioning bounds')
                src = os.open(name, os.O_RDONLY | os.O_NOFOLLOW, dir_fd=src_fd)
                dst = os.open(name, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=dst_fd)
                try:
                    opened = os.fstat(src)
                    if (opened.st_dev, opened.st_ino, opened.st_size) != (value.st_dev, value.st_ino, value.st_size):
                        raise ValueError('source changed while provisioning')
                    remaining = value.st_size
                    while remaining:
                        chunk = os.read(src, min(remaining, 1 << 20))
                        if not chunk:
                            raise ValueError('source changed while copying')
                        remaining -= len(chunk)
                        view = memoryview(chunk)
                        while view:
                            view = view[os.write(dst, view):]
                    if os.read(src, 1):
                        raise ValueError('source grew while copying')
                    os.fsync(dst)
                finally:
                    os.close(src)
                    os.close(dst)
            else:
                raise ValueError('sample contains a link or special file')
        os.fsync(dst_fd)
    src = directory(source)
    try:
        visit(src, destination)
    finally:
        os.close(src)
    return {'files': files, 'bytes': total}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--destination', type=Path, required=True, help='new absent session directory')
    parser.add_argument('--world', type=Path, required=True, help='existing sample/import to copy as data')
    parser.add_argument('--editor', type=Path, help='trusted built editor executable; omit to provision only')
    parser.add_argument('--font-pack', type=Path, help='explicit absolute immutable font pack; required for editor launch')
    parser.add_argument('--demo-script', action='store_true')
    parser.add_argument('--reference', action='store_true', help='headless reference proof, no native acceptance')
    args = parser.parse_args()
    font_record = None
    if args.editor and args.font_pack is None:
        raise ValueError('editor launch requires explicit --font-pack')
    if args.font_pack is not None:
        import font_pack
        if not args.font_pack.is_absolute():
            raise ValueError('font pack must be an explicit absolute directory')
        manifest = font_pack.read_regular(args.font_pack / 'manifest.json', font_pack.MAX_MANIFEST)
        font_record = {'scope': 'candidate/local data byte verification only; no registry proof',
                       'registry_proof': False, 'path': str(args.font_pack),
                       **font_pack.verify(args.font_pack, manifest)}
    if sys.platform != 'linux':
        raise ValueError('checked prototype editing is supported only on Linux')
    # Resolve only the existing trusted parent. The new final component is
    # created exclusively, and is never silently adopted or chmodded.
    parent = args.destination.parent.resolve(strict=True)
    name = args.destination.name
    if not name or name in ('.', '..'):
        raise ValueError('invalid session name')
    parent_fd = directory(parent)
    held = []
    try:
        persistent_mount(parent_fd)
        os.mkdir(name, mode=0o700, dir_fd=parent_fd)
        task_fd = os.open(name, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=parent_fd)
        held.append(task_fd)
        ids = {'session': identity(task_fd)}
        children = {}
        for child in ('world', 'recovery', 'evidence'):
            os.mkdir(child, mode=0o700, dir_fd=task_fd)
            fd = os.open(child, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW, dir_fd=task_fd)
            held.append(fd)
            children[child] = fd
            ids[child] = identity(fd)
        if len({(v['device'], v['inode']) for v in ids.values()}) != 4:
            raise ValueError('provisioned directory identity collision')
        copied = copy_tree(args.world, children['world'])
        os.fsync(task_fd)
        os.fsync(parent_fd)
        receipt = {'format': 'tilewright.prototype-provision/1', 'platform': sys.platform,
                   'roots': ids, 'copy': copied, 'font_pack': font_record, 'scope': 'fresh distinct retained roots; no arbitrary-root guarantee'}
        receipt_fd = os.open('provision.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=task_fd)
        with os.fdopen(receipt_fd, 'w') as stream:
            json.dump(receipt, stream, indent=2)
            stream.write('\n')
            stream.flush()
            os.fsync(stream.fileno())
        os.fsync(task_fd)
        print(json.dumps({'session': str(parent / name), 'provision': receipt}), flush=True)
        # Revalidate the same retained FDs at use, after copying and syncing.
        for child, fd in children.items():
            if identity(fd) != ids[child]:
                raise ValueError('provisioned directory identity or mount changed before launch')
        if identity(task_fd) != ids['session']:
            raise ValueError('session identity or mount changed before launch')
        if args.editor:
            executable = args.editor.resolve(strict=True)
            command = [str(executable), 'prototype',
                       '--world-fd', str(children['world']),
                       '--recovery-fd', str(children['recovery']),
                       '--evidence-fd', str(children['evidence']),
                       '--font-pack', str(args.font_pack)]
            if args.demo_script:
                command.append('--demo-script')
            if args.reference:
                command.append('--reference')
            result = subprocess.run(command, pass_fds=tuple(children.values()), check=False)
            return result.returncode
        return 0
    finally:
        for fd in reversed(held):
            os.close(fd)
        os.close(parent_fd)


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (OSError, ValueError) as error:
        raise SystemExit(f'prototype provisioning refused; preserve any created state for inspection: {error}') from error
