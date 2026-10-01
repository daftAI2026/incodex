#!/usr/bin/env python3
# [INPUT]: 依赖独立实验钥匙串、系统 OpenSSL/clang/swiftc/codesign 与同目录原生探针；不访问官方应用
# [OUTPUT]: 提供私有稳定身份存储、证书绑定 DR、编号候选构建与只读计划
# [POS]: macos-ax-continuity 的实验后端；身份不随 v1/v2 或源码换代，未接产品安装器
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import plistlib
import secrets
import stat
import subprocess
from typing import Callable

BUNDLE_ID = 'com.daftai.incodex.ax-continuity-lab'
CANDIDATE_ID = 'INCODEX-V1.1-MAC-AX-RC1'
RUNTIME_BASE = '1.1.0'
SOURCE = Path(__file__).resolve().parent


@dataclass(frozen=True)
class Identity:
    root: Path
    certificate_sha1: str
    certificate_sha256: str


def private_directory(path: Path):
    path = path.absolute()
    # 检查全部祖先；不通过符号链接把证书或钥匙串发布到别处。
    for ancestor in reversed(path.parents):
        if ancestor.is_symlink():
            raise ValueError(f'symlink ancestor: {ancestor}')
        if not ancestor.exists():
            ancestor.mkdir(mode=0o700)
    if not path.exists() and not path.is_symlink():
        path.mkdir(mode=0o700)
    meta = path.lstat()
    if not stat.S_ISDIR(meta.st_mode) or meta.st_uid != os.getuid() or stat.S_IMODE(meta.st_mode) != 0o700:
        raise ValueError(f'not a private current-user directory: {path}')


def private_file(path: Path) -> bytes:
    meta = path.lstat()
    if not stat.S_ISREG(meta.st_mode) or meta.st_uid != os.getuid() or stat.S_IMODE(meta.st_mode) != 0o600:
        raise ValueError(f'not a private current-user file: {path}')
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW)
    try:
        observed = os.fstat(fd)
        if (meta.st_dev, meta.st_ino) != (observed.st_dev, observed.st_ino):
            raise ValueError('identity file changed during read')
        with os.fdopen(fd, 'rb', closefd=False) as stream:
            return stream.read()
    finally:
        os.close(fd)


def write_private(path: Path, data: bytes):
    temp = path.with_name(path.name + '.new')
    fd = os.open(temp, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600)
    try:
        with os.fdopen(fd, 'wb', closefd=False) as stream:
            stream.write(data)
            stream.flush()
            os.fsync(fd)
        os.replace(temp, path)
    finally:
        os.close(fd)
        if temp.exists():
            temp.unlink()


def ensure_identity(root: Path, generate: Callable[[Path], None]) -> Identity:
    private_directory(root)
    record_path = root / 'identity.json'
    if record_path.exists() or record_path.is_symlink():
        try:
            record = json.loads(private_file(record_path))
            certificate = private_file(root / 'certificate.der')
            private_file(root / 'identity.keychain-db')
            private_file(root / 'password')
        except (OSError, KeyError, json.JSONDecodeError) as error:
            raise ValueError('existing identity is incomplete; explicit recovery required') from error
        expected = {'schemaVersion': 1, 'certificateSha1': hashlib.sha1(certificate).hexdigest(),
                    'certificateSha256': hashlib.sha256(certificate).hexdigest()}
        if record != expected:
            raise ValueError('existing signing identity failed its certificate proof')
    else:
        if any(root.iterdir()):
            raise ValueError('partial identity exists; refusing silent regeneration')
        generate(root)
        certificate = private_file(root / 'certificate.der')
        private_file(root / 'identity.keychain-db')
        private_file(root / 'password')
        record = {'schemaVersion': 1, 'certificateSha1': hashlib.sha1(certificate).hexdigest(),
                  'certificateSha256': hashlib.sha256(certificate).hexdigest()}
        write_private(record_path, json.dumps(record, indent=2).encode() + b'\n')
    return Identity(root.resolve(), record['certificateSha1'], record['certificateSha256'])


def identity_root_for(candidate_root: Path) -> Path:
    # 对齐 Storage 的首次获权身份冻结：RC 目录只持有载荷，不拥有长期身份。
    return candidate_root.parent / 'stable-identity'


def requirement_for(identity: Identity) -> str:
    return f'designated => identifier "{BUNDLE_ID}" and certificate leaf = H"{identity.certificate_sha1}"'


def make_candidate(identity: Identity, generation: str, binary: bytes, source_commit: str) -> dict:
    if generation not in ('v1', 'v2'):
        raise ValueError('generation must be v1 or v2')
    return {'schemaVersion': 1, 'candidateId': CANDIDATE_ID,
            'runtimeBaseVersion': RUNTIME_BASE, 'generation': generation,
            'sourceCommit': source_commit, 'binarySha256': hashlib.sha256(binary).hexdigest(),
            'signingCertificateSha1': identity.certificate_sha1,
            'signingCertificateSha256': identity.certificate_sha256,
            'bundleId': BUNDLE_ID, 'productInstalled': False}


def run(command: list[str], timeout=60, include_stderr=False):
    result = subprocess.run(command, check=False, capture_output=True, text=True, timeout=timeout)
    if result.returncode:
        raise RuntimeError(f'{Path(command[0]).name} failed ({result.returncode}): {result.stderr[-1600:]}')
    return result.stdout + (result.stderr if include_stderr else '')


def compile_identity_store(root: Path) -> Path:
    binary = root / 'identity-store'
    run(['/usr/bin/clang', '-fobjc-arc', '-framework', 'Foundation', '-framework', 'Security',
         str(SOURCE / 'identity-store.m'), '-o', str(binary)])
    binary.chmod(0o700)
    return binary


def generate_identity(root: Path, store: Path):
    # 独立钥匙串；原生后端使用 dynamic preference domain，不修改用户 search list。
    password = root / 'password'
    write_private(password, secrets.token_hex(32).encode())
    config = root / 'certificate.cnf'
    write_private(config, b'''[req]
prompt=no
distinguished_name=dn
x509_extensions=code_signing
[dn]
CN=Incodex AX Continuity Local Lab
[code_signing]
basicConstraints=critical,CA:FALSE
keyUsage=critical,digitalSignature
extendedKeyUsage=critical,codeSigning
subjectKeyIdentifier=hash
''')
    run(['/usr/bin/openssl', 'req', '-new', '-x509', '-newkey', 'rsa:2048', '-nodes',
         '-days', '3650', '-config', str(config), '-keyout', str(root / 'private-key.pem'),
         '-out', str(root / 'certificate.pem')])
    run(['/usr/bin/openssl', 'x509', '-in', str(root / 'certificate.pem'), '-outform', 'DER',
         '-out', str(root / 'certificate.der')])
    run(['/usr/bin/openssl', 'pkcs12', '-export', '-inkey', str(root / 'private-key.pem'),
         '-in', str(root / 'certificate.pem'), '-out', str(root / 'identity.p12'),
         '-passout', f'file:{password}'])
    for name in ('private-key.pem', 'certificate.pem', 'certificate.der', 'identity.p12'):
        (root / name).chmod(0o600)
    run([str(store), 'create', str(root / 'identity.keychain-db'), str(password), str(root / 'identity.p12')])
    (root / 'identity.keychain-db').chmod(0o600)


def build(root: Path) -> dict:
    source_files = ['continuity.py', 'helper.swift', 'identity-store.m']
    changes = run(['/usr/bin/git', '-C', str(SOURCE), 'status', '--porcelain', '--', *source_files])
    if changes.strip():
        raise ValueError('commit lab source before freezing a numbered candidate')
    private_directory(root)
    lock = root / 'build.lock'
    lock.mkdir(mode=0o700)  # 不破坏已有锁，也不并发轮换签名身份。
    try:
        store = compile_identity_store(root)
        identity = ensure_identity(identity_root_for(root), lambda path: generate_identity(path, store))
        run([str(store), 'unlock', str(identity.root / 'identity.keychain-db'), str(identity.root / 'password')])
        commit = run(['/usr/bin/git', '-C', str(SOURCE), 'rev-parse', 'HEAD']).strip()
        source_digest = hashlib.sha256(b''.join((SOURCE / name).read_bytes() for name in
                                                ('continuity.py', 'helper.swift', 'identity-store.m'))).hexdigest()
        candidates = []
        for generation in ('v1', 'v2'):
            app = root / generation / 'Incodex AX Continuity Lab.app'
            if app.exists():
                raise ValueError(f'candidate already exists, refusing overwrite: {app}')
            macos = app / 'Contents' / 'MacOS'
            resources = app / 'Contents' / 'Resources'
            macos.mkdir(parents=True)
            resources.mkdir()
            binary = macos / 'incodex-ax-lab'
            command = ['/usr/bin/swiftc', '-O', '-framework', 'AppKit', '-framework', 'ApplicationServices',
                       str(SOURCE / 'helper.swift'), '-o', str(binary)]
            if generation == 'v2':
                command[1:1] = ['-D', 'GENERATION_TWO']
            run(command, timeout=180)
            info = {'CFBundleIdentifier': BUNDLE_ID, 'CFBundleName': 'Incodex AX Continuity Lab',
                    'CFBundleExecutable': binary.name, 'CFBundlePackageType': 'APPL',
                    'CFBundleShortVersionString': RUNTIME_BASE, 'CFBundleVersion': '1' if generation == 'v1' else '2',
                    'NSHighResolutionCapable': True, 'LSUIElement': False}
            (app / 'Contents' / 'Info.plist').write_bytes(plistlib.dumps(info))
            metadata = {'candidateId': CANDIDATE_ID, 'generation': generation, 'sourceCommit': commit,
                        'sourceSha256': source_digest, 'runtimeBaseVersion': RUNTIME_BASE}
            (resources / 'candidate.json').write_text(json.dumps(metadata, indent=2) + '\n')
            run(['/usr/bin/codesign', '--force', '--sign', identity.certificate_sha1,
                 '--keychain', str(identity.root / 'identity.keychain-db'), '--identifier', BUNDLE_ID,
                 '--requirements', requirement_for(identity), '--timestamp=none', str(app)])
            run(['/usr/bin/codesign', '--verify', '--strict', str(app)])
            run(['/usr/bin/codesign', '--verify', '--strict', '-R', '=' + requirement_for(identity).split('=>', 1)[1].strip(), str(app)])
            manifest = make_candidate(identity, generation, binary.read_bytes(), commit)
            manifest['sourceSha256'] = source_digest
            manifest['appPath'] = str(app)
            manifest['designatedRequirement'] = run(['/usr/bin/codesign', '-dr', '-', str(app)], include_stderr=True)
            write_private(root / f'{generation}-candidate.json', json.dumps(manifest, indent=2).encode() + b'\n')
            candidates.append(manifest)
        if candidates[0]['binarySha256'] == candidates[1]['binarySha256']:
            raise ValueError('v1/v2 bytes are identical: invalid continuity experiment')
        summary = {'candidateId': CANDIDATE_ID, 'runtimeBaseVersion': RUNTIME_BASE,
                   'productInstalled': False, 'tccContinuity': 'NOT_RUN', 'candidates': candidates}
        write_private(root / 'candidate.json', json.dumps(summary, indent=2).encode() + b'\n')
        return summary
    finally:
        lock.rmdir()


def main():
    parser = argparse.ArgumentParser(description='Non-shipped AX identity lab; never targets official Codex')
    parser.add_argument('--root', type=Path, default=Path.home() / '.incodex' / 'labs' / 'macos-ax-continuity' / CANDIDATE_ID)
    parser.add_argument('--dry-run', action='store_true')
    args = parser.parse_args()
    if args.dry_run:
        print(json.dumps({'candidateId': CANDIDATE_ID, 'root': str(args.root),
                          'plan': ['create private synthetic identity', 'build and sign v1/v2 lab apps'],
                          'officialAppMutation': False, 'runtimePublication': False, 'tccMutation': False}, indent=2))
        return
    print(json.dumps(build(args.root), indent=2))


if __name__ == '__main__':
    main()
