/*
 * [INPUT]: 依赖 Security.framework 与私有目录中的密码/P12 文件，服务显式身份注册/解锁及只读身份验证。
 * [OUTPUT]: 提供受限 codesign ACL 的私有 Keychain create/verify/unlock 一次性命令，不输出秘密。
 * [POS]: CLI 的本机签名身份后端；不在用户搜索列表注册、不设置证书信任、不承担 AX 权限。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static const size_t kMinimumPasswordBytes = 32;
static const size_t kMaximumPasswordBytes = 1024;
static const size_t kMaximumPkcs12Bytes = 8 * 1024 * 1024;
static const size_t kMaximumCertificateBytes = 64 * 1024;

static bool is_private_directory(NSString *path) {
    struct stat metadata;
    if (lstat(path.fileSystemRepresentation, &metadata) != 0) return false;
    return S_ISDIR(metadata.st_mode) && metadata.st_uid == geteuid() &&
           (metadata.st_mode & 0777) == 0700;
}

static bool is_private_regular_file(NSString *path, size_t minimum,
                                    size_t maximum, struct stat *observed) {
    struct stat metadata;
    if (lstat(path.fileSystemRepresentation, &metadata) != 0 ||
        !S_ISREG(metadata.st_mode) || metadata.st_uid != geteuid() ||
        (metadata.st_mode & 0777) != 0600 || metadata.st_nlink != 1 ||
        metadata.st_size < 0 || (uint64_t)metadata.st_size < minimum ||
        (uint64_t)metadata.st_size > maximum) {
        return false;
    }
    if (observed) *observed = metadata;
    return true;
}

static NSData *read_private_file(NSString *path, size_t minimum, size_t maximum) {
    struct stat before;
    if (!is_private_regular_file(path, minimum, maximum, &before)) return nil;
    int descriptor = open(path.fileSystemRepresentation, O_RDONLY | O_NOFOLLOW);
    if (descriptor < 0) return nil;
    struct stat opened;
    if (fstat(descriptor, &opened) != 0 || !S_ISREG(opened.st_mode) ||
        opened.st_uid != geteuid() || (opened.st_mode & 0777) != 0600 ||
        opened.st_nlink != 1 || opened.st_dev != before.st_dev ||
        opened.st_ino != before.st_ino || opened.st_size != before.st_size) {
        close(descriptor);
        return nil;
    }

    NSMutableData *data = [NSMutableData dataWithLength:(NSUInteger)opened.st_size];
    uint8_t *bytes = data.mutableBytes;
    size_t offset = 0;
    while (offset < (size_t)opened.st_size) {
        ssize_t count = read(descriptor, bytes + offset,
                             (size_t)opened.st_size - offset);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) {
            close(descriptor);
            return nil;
        }
        offset += (size_t)count;
    }
    close(descriptor);
    return data;
}

static bool valid_keychain_path(NSString *path) {
    if (!path.isAbsolutePath ||
        ![path.lastPathComponent isEqualToString:@"identity.keychain-db"] ||
        ![path.stringByDeletingLastPathComponent.lastPathComponent
            isEqualToString:@"macos-signing"] ||
        [path containsString:@"/login.keychain"] ||
        [path isEqualToString:@"/Library/Keychains/System.keychain"]) {
        return false;
    }
    return is_private_directory(path.stringByDeletingLastPathComponent);
}

static bool valid_certificate_path(NSString *path, NSString *keychain_path) {
    return [path.lastPathComponent isEqualToString:@"certificate.der"] &&
           [path.stringByDeletingLastPathComponent
               isEqualToString:keychain_path.stringByDeletingLastPathComponent] &&
           is_private_regular_file(path, 1, kMaximumCertificateBytes, NULL);
}

#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
static bool search_list_unchanged(CFArrayRef before) {
    CFArrayRef after = NULL;
    OSStatus status = SecKeychainCopySearchList(&after);
    bool unchanged = status == errSecSuccess && before && after &&
                     CFEqual(before, after);
    if (after) CFRelease(after);
    return unchanged;
}

static int run_create(NSString *keychain_path, NSString *pkcs12_path,
                      NSData *password) {
    if (access(keychain_path.fileSystemRepresentation, F_OK) == 0 || errno != ENOENT) {
        return 67;
    }
    NSData *bundle = read_private_file(pkcs12_path, 1, kMaximumPkcs12Bytes);
    if (!bundle) return 65;
    NSString *passphrase = [[NSString alloc] initWithData:password
                                                   encoding:NSUTF8StringEncoding];
    if (!passphrase) return 65;

    CFArrayRef before = NULL;
    OSStatus status = SecKeychainCopySearchList(&before);
    if (status != errSecSuccess || !before) {
        if (before) CFRelease(before);
        return 66;
    }

    (void)SecKeychainSetUserInteractionAllowed(false);
    SecKeychainRef keychain = NULL;
    status = SecKeychainCreate(keychain_path.fileSystemRepresentation,
                               (UInt32)password.length, password.bytes, false,
                               NULL, &keychain);
    if (status == errSecSuccess) {
        SecKeychainSettings settings = {
            SEC_KEYCHAIN_SETTINGS_VERS1,
            true,
            true,
            300,
        };
        status = SecKeychainSetSettings(keychain, &settings);
    }

    SecTrustedApplicationRef signer = NULL;
    SecAccessRef access = NULL;
    CFArrayRef trusted = NULL;
    if (status == errSecSuccess) {
        status = SecTrustedApplicationCreateFromPath("/usr/bin/codesign", &signer);
    }
    if (status == errSecSuccess) {
        const void *applications[] = {signer};
        trusted = CFArrayCreate(kCFAllocatorDefault, applications, 1,
                                &kCFTypeArrayCallBacks);
        if (!trusted) status = errSecAllocate;
    }
    if (status == errSecSuccess) {
        status = SecAccessCreate(CFSTR("Incodex Local Signing Identity"),
                                 trusted, &access);
    }
    if (status == errSecSuccess) {
        SecExternalFormat format = kSecFormatPKCS12;
        SecExternalItemType type = kSecItemTypeAggregate;
        SecItemImportExportKeyParameters parameters = {0};
        parameters.version = SEC_KEY_IMPORT_EXPORT_PARAMS_VERSION;
        parameters.passphrase = (__bridge CFTypeRef)passphrase;
        parameters.accessRef = access;
        status = SecItemImport((__bridge CFDataRef)bundle,
                               CFSTR("identity.p12"), &format, &type, 0,
                               &parameters, keychain, NULL);
    }

    if (trusted) CFRelease(trusted);
    if (access) CFRelease(access);
    if (signer) CFRelease(signer);
    if (keychain) CFRelease(keychain);

    if (status == errSecSuccess &&
        chmod(keychain_path.fileSystemRepresentation, 0600) != 0) {
        status = errSecIO;
    }
    struct stat keychain_metadata;
    if (status == errSecSuccess &&
        !is_private_regular_file(keychain_path, 1, SIZE_MAX,
                                 &keychain_metadata)) {
        status = errSecInvalidItemRef;
    }
    bool unchanged = search_list_unchanged(before);
    CFRelease(before);
    if (!unchanged) return 70;
    return status == errSecSuccess ? 0 : 69;
}

static int run_unlock(NSString *keychain_path, NSData *password) {
    if (!is_private_regular_file(keychain_path, 1, SIZE_MAX, NULL)) return 68;
    CFArrayRef before = NULL;
    OSStatus status = SecKeychainCopySearchList(&before);
    if (status != errSecSuccess || !before) {
        if (before) CFRelease(before);
        return 66;
    }

    (void)SecKeychainSetUserInteractionAllowed(false);
    SecKeychainRef keychain = NULL;
    status = SecKeychainOpen(keychain_path.fileSystemRepresentation, &keychain);
    if (status == errSecSuccess) {
        status = SecKeychainUnlock(keychain, (UInt32)password.length,
                                   password.bytes, true);
    }
    if (keychain) CFRelease(keychain);
    bool unchanged = search_list_unchanged(before);
    CFRelease(before);
    if (!unchanged) return 70;
    return status == errSecSuccess ? 0 : 69;
}

static int run_verify(NSString *keychain_path, NSString *certificate_path) {
    NSData *expected_der = read_private_file(certificate_path, 1,
                                              kMaximumCertificateBytes);
    if (!expected_der) return 65;
    CFArrayRef before = NULL;
    OSStatus status = SecKeychainCopySearchList(&before);
    if (status != errSecSuccess || !before) {
        if (before) CFRelease(before);
        return 66;
    }

    (void)SecKeychainSetUserInteractionAllowed(false);
    SecKeychainRef keychain = NULL;
    status = SecKeychainOpen(keychain_path.fileSystemRepresentation, &keychain);
    bool certificate_found = false;
    bool private_key_found = false;
    if (status == errSecSuccess) {
        SecKeychainSearchRef search = NULL;
        status = SecKeychainSearchCreateFromAttributes(
            keychain, kSecCertificateItemClass, NULL, &search);
        if (status == errSecSuccess) {
            SecKeychainItemRef item = NULL;
            while ((status = SecKeychainSearchCopyNext(search, &item)) == errSecSuccess) {
                CFDataRef actual_der = SecCertificateCopyData((SecCertificateRef)item);
                if (actual_der && CFEqual(actual_der, (__bridge CFDataRef)expected_der)) {
                    certificate_found = true;
                    SecIdentityRef identity = NULL;
                    OSStatus identity_status = SecIdentityCreateWithCertificate(
                        keychain, (SecCertificateRef)item, &identity);
                    private_key_found = identity_status == errSecSuccess && identity;
                    if (identity) CFRelease(identity);
                }
                if (actual_der) CFRelease(actual_der);
                CFRelease(item);
                item = NULL;
                if (certificate_found) break;
            }
            if (search) CFRelease(search);
        }
    }
    if (keychain) CFRelease(keychain);
    bool unchanged = search_list_unchanged(before);
    CFRelease(before);
    if (!unchanged) return 70;
    if (certificate_found && private_key_found) return 0;
    return 69;
}
#pragma clang diagnostic pop

static void secure_zero(void *buffer, size_t length) {
    volatile uint8_t *cursor = buffer;
    while (length-- > 0) *cursor++ = 0;
}

static int run(int argc, const char **argv) {
    bool create = argc == 5 && strcmp(argv[1], "create") == 0;
    bool unlock = argc == 4 && strcmp(argv[1], "unlock") == 0;
    bool verify = argc == 4 && strcmp(argv[1], "verify") == 0;
    if (!create && !unlock && !verify) return 64;
    NSString *mode = @(argv[1]);
    NSString *keychain_path = @(argv[2]);
    if (!valid_keychain_path(keychain_path)) return 67;
    int status = -1;
    if (create) {
        NSString *password_path = @(argv[3]);
        if (![password_path.lastPathComponent isEqualToString:@"identity.password"] ||
            ![password_path.stringByDeletingLastPathComponent
                isEqualToString:keychain_path.stringByDeletingLastPathComponent]) {
            return 67;
        }
        NSMutableData *password = [read_private_file(password_path,
                                                    kMinimumPasswordBytes,
                                                    kMaximumPasswordBytes) mutableCopy];
        if (!password) return 65;
        NSString *pkcs12_path = @(argv[4]);
        NSString *staging_directory = pkcs12_path.stringByDeletingLastPathComponent;
        NSString *identity_directory = keychain_path.stringByDeletingLastPathComponent;
        if (![staging_directory.lastPathComponent hasPrefix:@".staging-"] ||
            ![staging_directory.stringByDeletingLastPathComponent
                isEqualToString:identity_directory] ||
            ![pkcs12_path.lastPathComponent isEqualToString:@"identity.p12"]) {
            status = 67;
        } else {
            if (!is_private_directory(staging_directory)) {
                status = 67;
            } else {
                status = run_create(keychain_path, pkcs12_path, password);
            }
        }
        secure_zero(password.mutableBytes, password.length);
    } else if (unlock) {
        NSString *password_path = @(argv[3]);
        if (![password_path.lastPathComponent isEqualToString:@"identity.password"] ||
            ![password_path.stringByDeletingLastPathComponent
                isEqualToString:keychain_path.stringByDeletingLastPathComponent]) {
            return 67;
        }
        NSMutableData *password = [read_private_file(password_path,
                                                    kMinimumPasswordBytes,
                                                    kMaximumPasswordBytes) mutableCopy];
        if (!password) return 65;
        status = run_unlock(keychain_path, password);
        secure_zero(password.mutableBytes, password.length);
    } else if (verify) {
        NSString *certificate_path = @(argv[3]);
        if (!valid_certificate_path(certificate_path, keychain_path)) return 65;
        status = run_verify(keychain_path, certificate_path);
    }
    if (status < 0) return 64;
    if (status != 0) return status;

    const char *result = [mode isEqualToString:@"create"]
        ? "{\"privateIdentityReady\":true}\n"
        : ([mode isEqualToString:@"verify"]
            ? "{\"privateIdentityVerified\":true}\n"
            : "{\"privateIdentityUnlocked\":true}\n");
    size_t length = strlen(result);
    size_t offset = 0;
    while (offset < length) {
        ssize_t count = write(STDOUT_FILENO, result + offset, length - offset);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return 74;
        offset += (size_t)count;
    }
    return 0;
}

int main(int argc, const char **argv) {
    @autoreleasepool {
        return run(argc, argv);
    }
}
