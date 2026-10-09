#!/usr/bin/env python3
# [INPUT]: 依赖系统 clang/OpenSSL/Security 与原生身份后端；仅操作临时 synthetic 钥匙串
# [OUTPUT]: 提供私有钥匙串 create/import 和用户 search list 不变的真实工具链回归
# [POS]: macos-ax-continuity 的 native 门，不授予 TCC、不调用真实登录 Keychain 查询
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
import os
from pathlib import Path
import tempfile
import unittest
from continuity import compile_identity_store, ensure_identity, generate_identity, run


class NativeIdentityContract(unittest.TestCase):
    def test_private_identity_can_be_created_without_search_list_mutation(self):
        before = run(['/usr/bin/security', 'list-keychains', '-d', 'user'])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            root.chmod(0o700)
            store = compile_identity_store(root)
            try:
                identity = ensure_identity(root / 'identity', lambda path: generate_identity(path, store))
                self.assertEqual(len(identity.certificate_sha256), 64)
                self.assertEqual(os.stat(identity.root / 'identity.keychain-db').st_mode & 0o777, 0o600)
                run([str(store), 'unlock', str(identity.root / 'identity.keychain-db'), str(identity.root / 'password')])
            finally:
                after = run(['/usr/bin/security', 'list-keychains', '-d', 'user'])
                self.assertEqual(before, after, 'private identity changed global search list')


if __name__ == '__main__':
    unittest.main()
