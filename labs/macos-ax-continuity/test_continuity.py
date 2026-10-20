#!/usr/bin/env python3
# [INPUT]: 依赖实验后端的身份存储与候选清单；只使用临时 synthetic 文件，不访问真实钥匙串或 TCC
# [OUTPUT]: 提供稳定身份复用、篡改拒绝、私有存储与编号的失败先行测试
# [POS]: macos-ax-continuity 的策略测试；签名、真实 AX 与跨代授权另由实机验证
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
import hashlib
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import subprocess
import continuity

from continuity import ensure_identity, make_candidate, requirement_for


class ContinuityContract(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve() / 'identity'
        self.calls = 0

    def generate(self, root):
        self.calls += 1
        for name, content in (
            ('certificate.der', b'synthetic certificate'),
            ('identity.keychain-db', b'synthetic keychain'),
            ('password', b'synthetic generated password'),
        ):
            path = root / name
            path.write_bytes(content)
            path.chmod(0o600)

    def test_new_candidate_does_not_rotate_first_identity(self):
        first = ensure_identity(self.root, self.generate)
        second = ensure_identity(self.root, lambda _: self.fail('identity rotated'))
        self.assertEqual(first, second)
        self.assertEqual(self.calls, 1)
        self.assertEqual(os.stat(self.root).st_mode & 0o777, 0o700)

    def test_certificate_change_fails_without_regeneration(self):
        ensure_identity(self.root, self.generate)
        (self.root / 'certificate.der').write_bytes(b'replaced certificate')
        with self.assertRaises(ValueError):
            ensure_identity(self.root, lambda _: self.fail('unsafe repair'))

    def test_missing_keychain_fails_without_silent_identity_rotation(self):
        ensure_identity(self.root, self.generate)
        (self.root / 'identity.keychain-db').unlink()
        with self.assertRaises(ValueError):
            ensure_identity(self.root, lambda _: self.fail('unsafe repair'))

    def test_symlink_identity_is_rejected(self):
        external = Path(self.temp.name).resolve() / 'external'
        external.mkdir()
        self.root.symlink_to(external, target_is_directory=True)
        with self.assertRaises(ValueError):
            ensure_identity(self.root, lambda _: self.fail('followed symlink'))

    def test_requirement_binds_identifier_and_certificate_not_identifier_only(self):
        identity = ensure_identity(self.root, self.generate)
        requirement = requirement_for(identity)
        self.assertIn('identifier "com.daftai.incodex.ax-continuity-lab"', requirement)
        self.assertIn('certificate leaf = H"', requirement)
        self.assertIn(hashlib.sha1(b'synthetic certificate').hexdigest(), requirement)
        self.assertNotIn('2DC432GLL2', requirement)

    def test_candidates_record_generation_bytes_and_runtime_base(self):
        identity = ensure_identity(self.root, self.generate)
        first = make_candidate(identity, 'v1', b'first build', 'test-source')
        second = make_candidate(identity, 'v2', b'different build', 'test-source')
        self.assertEqual(first['candidateId'], 'INCODEX-V1.1-MAC-AX-RC1')
        self.assertEqual(first['runtimeBaseVersion'], '1.1.0')
        self.assertEqual(first['signingCertificateSha1'], second['signingCertificateSha1'])
        self.assertNotEqual(first['binarySha256'], second['binarySha256'])
        self.assertEqual(first['generation'], 'v1')
        self.assertEqual(second['generation'], 'v2')
        self.assertFalse(first['productInstalled'])

    def test_rc_change_keeps_identity_outside_candidate_directory(self):
        first = continuity.identity_root_for(Path(self.temp.name).resolve() / 'RC1')
        second = continuity.identity_root_for(Path(self.temp.name).resolve() / 'RC2')
        self.assertEqual(first, second)
        self.assertEqual(first.name, 'stable-identity')

    def test_symlink_ancestor_is_rejected_before_creation(self):
        external = Path(self.temp.name).resolve() / 'external'
        external.mkdir(mode=0o700)
        alias = Path(self.temp.name).resolve() / 'alias'
        alias.symlink_to(external, target_is_directory=True)
        with self.assertRaises(ValueError):
            continuity.private_directory(alias / 'new-private')
        self.assertFalse((external / 'new-private').exists())

    def test_codesign_requirement_is_captured_from_stderr(self):
        result = subprocess.CompletedProcess(['codesign'], 0, '', 'designated => synthetic')
        with patch('continuity.subprocess.run', return_value=result):
            actual = continuity.run(['codesign', '-dr', '-', 'synthetic.app'], include_stderr=True)
        self.assertEqual(actual, 'designated => synthetic')

    def test_codesign_inline_requirement_has_expression_prefix(self):
        identity = ensure_identity(self.root, self.generate)
        self.assertEqual(continuity.signing_requirement_argument(identity), '=' + requirement_for(identity))

    def test_invalid_generation_fails_closed(self):
        identity = ensure_identity(self.root, self.generate)
        with self.assertRaises(ValueError):
            make_candidate(identity, 'official', b'build', 'test-source')


if __name__ == '__main__':
    unittest.main()
