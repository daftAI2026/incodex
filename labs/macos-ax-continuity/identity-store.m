/*
 * [INPUT]: 依赖 Security.framework 的独立私有钥匙串与受限代码签名 ACL，输入仅来自实验私有目录
 * [OUTPUT]: 提供 create/unlock 一次性身份后端，不返回密码或私钥，不修改用户钥匙串搜索列表
 * [POS]: macos-ax-continuity 的签名身份初始化器；不是 AX 权限代理，不加入 Runtime 或官方包
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#include <limits.h>
#include <sys/stat.h>
#include <unistd.h>

static NSData *privateData(NSString *path) {
    struct stat st;
    if (lstat(path.fileSystemRepresentation, &st) != 0 || !S_ISREG(st.st_mode) ||
        st.st_uid != getuid() || (st.st_mode & 0777) != 0600) return nil;
    return [NSData dataWithContentsOfFile:path options:NSDataReadingUncached error:nil];
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 4 && argc != 5) return 64;
        NSString *mode = @(argv[1]);
        NSString *keychainPath = @(argv[2]);
        NSData *password = privateData(@(argv[3]));
        if (!password || password.length == 0 || password.length > 1024) return 65;
        // Apple StorageManager 排除私有钥匙串的自动 search-list 发布；dynamic 不是可写偏好域。
        // 不调用 SetPreferenceDomain/SetSearchList；前后只读核验，全局偏好改变则失败。
        CFArrayRef before = NULL;
        OSStatus status = SecKeychainCopySearchList(&before);
        if (status != errSecSuccess) return 66;
        (void)SecKeychainSetUserInteractionAllowed(false);
        SecKeychainRef keychain = NULL;
        if ([keychainPath containsString:@"/login.keychain"] ||
            [keychainPath isEqualToString:@"/Library/Keychains/System.keychain"]) return 67;
        if ([mode isEqualToString:@"create"] && argc == 5) {
            if ([[NSFileManager defaultManager] fileExistsAtPath:keychainPath]) return 67;
            status = SecKeychainCreate(keychainPath.fileSystemRepresentation,
                                      (UInt32)password.length, password.bytes, false, NULL, &keychain);
            if (status == errSecSuccess) {
                SecKeychainSettings settings = {SEC_KEYCHAIN_SETTINGS_VERS1, false, false, INT_MAX};
                status = SecKeychainSetSettings(keychain, &settings);
            }
            SecTrustedApplicationRef signer = NULL;
            SecAccessRef access = NULL;
            if (status == errSecSuccess) status = SecTrustedApplicationCreateFromPath("/usr/bin/codesign", &signer);
            if (status == errSecSuccess) {
                const void *values[] = {signer};
                CFArrayRef trusted = CFArrayCreate(NULL, values, 1, &kCFTypeArrayCallBacks);
                status = SecAccessCreate(CFSTR("Incodex AX Lab Signing Identity"), trusted, &access);
                CFRelease(trusted);
            }
            NSData *bundle = privateData(@(argv[4]));
            NSString *phrase = [[NSString alloc] initWithData:password encoding:NSUTF8StringEncoding];
            if (status == errSecSuccess && (!bundle || !phrase)) status = errSecParam;
            if (status == errSecSuccess) {
                SecExternalFormat format = kSecFormatPKCS12;
                SecExternalItemType type = kSecItemTypeAggregate;
                SecItemImportExportKeyParameters parameters = {0};
                parameters.version = SEC_KEY_IMPORT_EXPORT_PARAMS_VERSION;
                parameters.passphrase = (__bridge CFTypeRef)phrase;
                parameters.accessRef = access;
                status = SecItemImport((__bridge CFDataRef)bundle, CFSTR("identity.p12"),
                                               &format, &type, 0, &parameters, keychain, NULL);
            }
            if (access) CFRelease(access);
            if (signer) CFRelease(signer);
        } else if ([mode isEqualToString:@"unlock"] && argc == 4) {
            if (!privateData(keychainPath)) return 68;
            status = SecKeychainOpen(keychainPath.fileSystemRepresentation, &keychain);
        } else {
            return 64;
        }
        if (status == errSecSuccess) {
            status = SecKeychainUnlock(keychain, (UInt32)password.length, password.bytes, true);
        }
        CFArrayRef after = NULL;
        OSStatus searchStatus = SecKeychainCopySearchList(&after);
        BOOL unchanged = searchStatus == errSecSuccess && before && after && CFEqual(before, after);
        if (before) CFRelease(before);
        if (after) CFRelease(after);
        if (keychain) CFRelease(keychain);
        if (!unchanged) {
            fputs("Private identity search-list invariant failed; no global repair attempted\n", stderr);
            return 70;
        }
        if (status != errSecSuccess) {
            fprintf(stderr, "Private identity operation failed: %d\n", (int)status);
            return 1;
        }
        puts("{\"privateIdentityReady\":true}");
        return 0;
    }
}
