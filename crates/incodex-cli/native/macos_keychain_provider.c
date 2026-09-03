/**
 * [INPUT]: 依赖 Security.framework 的 SecItemCopyMatching、内容寻址固定 helper 与 fishhook 符号重绑定
 * [OUTPUT]: 对 Codex Storage Key 精确查询提供有界 helper 读取，并在 helper 不可用时回退官方 Security 路径
 * [POS]: incodex-cli/native 的 Keychain 适配层，只接管单一查询且不得让可选增强破坏官方存储功能
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#include <CoreFoundation/CoreFoundation.h>
#include <CommonCrypto/CommonDigest.h>
#include <Security/Security.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <pwd.h>
#include <signal.h>
#include <spawn.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include "fishhook.h"

static const size_t kMaximumHelperOutput = 4096;
// provider 位于 Codex 同步 Keychain 调用中，绝不能把用户交互等待塞进主线程。
// 需要密码的授权只允许由显式 `incodex install` 在进程外预先完成。
static const int64_t kHelperTimeoutMilliseconds = 2000;
typedef OSStatus (*CopyMatching)(CFDictionaryRef, CFTypeRef *);

#ifndef INCODEX_KEYCHAIN_HELPER_SHA256
#define INCODEX_KEYCHAIN_HELPER_SHA256 ""
#endif

static void __attribute__((unused))
secure_zero(unsigned char *bytes, size_t length) {
    volatile unsigned char *cursor = bytes;
    while (length-- > 0) *cursor++ = 0;
}

static int64_t monotonic_milliseconds(void) {
    struct timespec now = {0};
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -1;
    return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}

static void terminate_child(pid_t child) {
    if (child <= 0) return;
    (void)kill(-child, SIGKILL);
    (void)kill(child, SIGKILL);
    while (waitpid(child, NULL, 0) < 0 && errno == EINTR) {
    }
}

static int __attribute__((unused))
run_helper(const char *path, unsigned char *output, size_t capacity,
           size_t *length) {
    if (!path || path[0] != '/' || !output || !length ||
        capacity < kMaximumHelperOutput) {
        return 64;
    }
    *length = 0;

    int pipe_fds[2] = {-1, -1};
    if (pipe(pipe_fds) != 0) return 70;
    (void)fcntl(pipe_fds[0], F_SETFD, FD_CLOEXEC);
    (void)fcntl(pipe_fds[1], F_SETFD, FD_CLOEXEC);

    posix_spawn_file_actions_t actions;
    posix_spawnattr_t attributes;
    if (posix_spawn_file_actions_init(&actions) != 0) {
        close(pipe_fds[0]);
        close(pipe_fds[1]);
        return 70;
    }
    if (posix_spawnattr_init(&attributes) != 0) {
        posix_spawn_file_actions_destroy(&actions);
        close(pipe_fds[0]);
        close(pipe_fds[1]);
        return 70;
    }
    (void)posix_spawn_file_actions_adddup2(
        &actions, pipe_fds[1], STDOUT_FILENO);
    (void)posix_spawn_file_actions_addclose(&actions, pipe_fds[0]);
    (void)posix_spawn_file_actions_addclose(&actions, pipe_fds[1]);
    (void)posix_spawn_file_actions_addopen(
        &actions, STDIN_FILENO, "/dev/null", O_RDONLY, 0);
    (void)posix_spawn_file_actions_addopen(
        &actions, STDERR_FILENO, "/dev/null", O_WRONLY, 0);

    short flags = POSIX_SPAWN_CLOEXEC_DEFAULT | POSIX_SPAWN_SETPGROUP;
    (void)posix_spawnattr_setflags(&attributes, flags);
    (void)posix_spawnattr_setpgroup(&attributes, 0);
    char *const arguments[] = {(char *)path, NULL};
    char *const environment[] = {NULL};
    pid_t child = -1;
    int spawn_status = posix_spawn(
        &child, path, &actions, &attributes, arguments, environment);
    posix_spawnattr_destroy(&attributes);
    posix_spawn_file_actions_destroy(&actions);
    close(pipe_fds[1]);
    if (spawn_status != 0) {
        close(pipe_fds[0]);
        return 70;
    }

    int file_flags = fcntl(pipe_fds[0], F_GETFL, 0);
    if (file_flags < 0 ||
        fcntl(pipe_fds[0], F_SETFL, file_flags | O_NONBLOCK) != 0) {
        close(pipe_fds[0]);
        terminate_child(child);
        return 70;
    }

    int64_t started = monotonic_milliseconds();
    if (started < 0) {
        close(pipe_fds[0]);
        terminate_child(child);
        return 70;
    }
    int64_t deadline = started + kHelperTimeoutMilliseconds;
    size_t used = 0;
    bool reached_eof = false;
    bool child_exited = false;
    int child_status = 0;

    while (!reached_eof || !child_exited) {
        int64_t now = monotonic_milliseconds();
        if (now < 0 || now >= deadline) {
            close(pipe_fds[0]);
            terminate_child(child);
            memset(output, 0, capacity);
            return 72;
        }

        struct pollfd descriptor = {
            .fd = pipe_fds[0], .events = POLLIN | POLLHUP, .revents = 0};
        int remaining = (int)(deadline - now);
        int poll_status = reached_eof ? 0 : poll(&descriptor, 1, remaining);
        if (poll_status < 0 && errno != EINTR) {
            close(pipe_fds[0]);
            terminate_child(child);
            memset(output, 0, capacity);
            return 70;
        }

        if (!reached_eof && poll_status > 0 &&
            (descriptor.revents & (POLLIN | POLLHUP))) {
            unsigned char chunk[512];
            for (;;) {
                ssize_t count = read(pipe_fds[0], chunk, sizeof(chunk));
                if (count > 0) {
                    size_t count_size = (size_t)count;
                    if (used > kMaximumHelperOutput - count_size) {
                        memset(chunk, 0, sizeof(chunk));
                        close(pipe_fds[0]);
                        terminate_child(child);
                        memset(output, 0, capacity);
                        return 71;
                    }
                    memcpy(output + used, chunk, count_size);
                    used += count_size;
                    memset(chunk, 0, sizeof(chunk));
                    continue;
                }
                if (count == 0) reached_eof = true;
                if (count < 0 && errno != EAGAIN && errno != EWOULDBLOCK &&
                    errno != EINTR) {
                    close(pipe_fds[0]);
                    terminate_child(child);
                    memset(output, 0, capacity);
                    return 70;
                }
                break;
            }
        }

        if (!child_exited) {
            pid_t waited = waitpid(child, &child_status, WNOHANG);
            if (waited == child) {
                child_exited = true;
            } else if (waited < 0 && errno != EINTR) {
                close(pipe_fds[0]);
                terminate_child(child);
                memset(output, 0, capacity);
                return 70;
            }
        }
    }
    close(pipe_fds[0]);

    if (!WIFEXITED(child_status)) {
        memset(output, 0, capacity);
        return 70;
    }
    int exit_status = WEXITSTATUS(child_status);
    if (exit_status != 0 || used == 0) {
        memset(output, 0, capacity);
        return exit_status == 44 ? 44 : 70;
    }
    *length = used;
    return 0;
}

static bool dictionary_boolean_is(CFDictionaryRef query, CFTypeRef key,
                                  bool expected, bool absent_is_false) {
    CFTypeRef value = CFDictionaryGetValue(query, key);
    if (!value) return absent_is_false && !expected;
    if (CFGetTypeID(value) != CFBooleanGetTypeID()) return false;
    return CFBooleanGetValue((CFBooleanRef)value) == expected;
}

static bool __attribute__((unused))
is_exact_storage_data_query(CFDictionaryRef query, CFTypeRef *result) {
    if (!query || !result || CFGetTypeID(query) != CFDictionaryGetTypeID()) {
        return false;
    }
    CFTypeRef item_class = CFDictionaryGetValue(query, kSecClass);
    CFTypeRef service = CFDictionaryGetValue(query, kSecAttrService);
    CFTypeRef account = CFDictionaryGetValue(query, kSecAttrAccount);
    CFTypeRef match_limit = CFDictionaryGetValue(query, kSecMatchLimit);
    if (!item_class || !CFEqual(item_class, kSecClassGenericPassword) ||
        !service || CFGetTypeID(service) != CFStringGetTypeID() ||
        !CFEqual(service, CFSTR("Codex Storage Key")) || !account ||
        CFGetTypeID(account) != CFStringGetTypeID() ||
        !CFEqual(account, CFSTR("Codex")) || !match_limit ||
        !CFEqual(match_limit, kSecMatchLimitOne)) {
        return false;
    }
    return dictionary_boolean_is(query, kSecReturnData, true, false) &&
           dictionary_boolean_is(query, kSecReturnAttributes, false, true) &&
           dictionary_boolean_is(query, kSecReturnRef, false, true) &&
           dictionary_boolean_is(query, kSecReturnPersistentRef, false, true);
}

static OSStatus __attribute__((unused))
copy_matching_with_helper(CFDictionaryRef query, CFTypeRef *result,
                          const char *helper_path, CopyMatching original) {
    if (!is_exact_storage_data_query(query, result)) {
        return original ? original(query, result) : errSecParam;
    }

    *result = NULL;
    unsigned char helper_output[4096] = {0};
    size_t helper_output_length = 0;
    int helper_status = run_helper(helper_path, helper_output,
                                   sizeof(helper_output),
                                   &helper_output_length);
    if (helper_status != 0) {
        memset(helper_output, 0, sizeof(helper_output));
        // helper 是增强路径，不是官方存储的单点故障。身份校验、缺项、
        // 超时或执行失败时都回到原始 Security 实现，保留 Codex 的原生
        // 解锁、创建与错误语义；代价至多是系统再次显示官方授权提示。
        return original ? original(query, result) : errSecParam;
    }

    CFDataRef data = CFDataCreate(kCFAllocatorDefault, helper_output,
                                  (CFIndex)helper_output_length);
    memset(helper_output, 0, sizeof(helper_output));
    if (!data) return errSecAllocate;
    *result = data;
    return errSecSuccess;
}

#ifdef INCODEX_TESTING
static CopyMatching original_copy_matching = NULL;

static OSStatus replacement_copy_matching(CFDictionaryRef query,
                                           CFTypeRef *result) {
    const char *helper = getenv("INCODEX_KEYCHAIN_TEST_HELPER");
    return copy_matching_with_helper(query, result, helper,
                                     original_copy_matching);
}

__attribute__((constructor)) static void install_test_rebinding(void) {
    struct rebinding binding = {
        .name = "SecItemCopyMatching",
        .replacement = (void *)replacement_copy_matching,
        .replaced = (void **)&original_copy_matching,
    };
    (void)rebind_symbols(&binding, 1);
}

__attribute__((visibility("default")))
int incodex_key_provider_test_run_helper(const char *path,
                                         unsigned char *output,
                                         size_t capacity, size_t *length) {
    return run_helper(path, output, capacity, length);
}

__attribute__((visibility("default")))
OSStatus incodex_key_provider_test_copy_matching(
    CFDictionaryRef query, CFTypeRef *result, const char *helper,
    CopyMatching original) {
    return copy_matching_with_helper(query, result, helper, original);
}
#else
static CopyMatching original_copy_matching = NULL;

static bool helper_hash_matches(int descriptor) {
    unsigned char observed[CC_SHA256_DIGEST_LENGTH] = {0};
    CC_SHA256_CTX context;
    if (CC_SHA256_Init(&context) != 1) return false;

    unsigned char buffer[8192] = {0};
    for (;;) {
        ssize_t count = read(descriptor, buffer, sizeof(buffer));
        if (count > 0) {
            if (CC_SHA256_Update(&context, buffer, (CC_LONG)count) != 1) {
                secure_zero(buffer, sizeof(buffer));
                return false;
            }
            continue;
        }
        if (count < 0 && errno == EINTR) continue;
        if (count < 0) {
            secure_zero(buffer, sizeof(buffer));
            return false;
        }
        break;
    }
    secure_zero(buffer, sizeof(buffer));
    if (CC_SHA256_Final(observed, &context) != 1) return false;

    static const char hex[] = "0123456789abcdef";
    char encoded[CC_SHA256_DIGEST_LENGTH * 2 + 1] = {0};
    for (size_t index = 0; index < sizeof(observed); ++index) {
        encoded[index * 2] = hex[observed[index] >> 4];
        encoded[index * 2 + 1] = hex[observed[index] & 0x0f];
    }
    secure_zero(observed, sizeof(observed));
    bool matches = strcmp(encoded, INCODEX_KEYCHAIN_HELPER_SHA256) == 0;
    secure_zero((unsigned char *)encoded, sizeof(encoded));
    return matches;
}

static bool resolve_verified_helper(char *path, size_t capacity) {
#ifdef INCODEX_KEYCHAIN_TEST_HOME
    const char *home = INCODEX_KEYCHAIN_TEST_HOME;
#else
    struct passwd password = {0};
    struct passwd *found = NULL;
    char password_buffer[4096] = {0};
    if (getpwuid_r(getuid(), &password, password_buffer,
                   sizeof(password_buffer), &found) != 0 ||
        !found || !found->pw_dir) {
        return false;
    }
    const char *home = found->pw_dir;
#endif
    int written = snprintf(
        path, capacity,
        "%s/.incodex/helpers/macos-keychain/%s/incodex-keychain-helper",
        home, INCODEX_KEYCHAIN_HELPER_SHA256);
    if (written <= 0 || (size_t)written >= capacity) return false;

    int descriptor = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0) return false;
    struct stat metadata = {0};
    bool valid = fstat(descriptor, &metadata) == 0 &&
                 S_ISREG(metadata.st_mode) && metadata.st_uid == geteuid() &&
                 (metadata.st_mode & 0777) == 0700 && metadata.st_nlink == 1 &&
                 helper_hash_matches(descriptor);
    close(descriptor);
    if (!valid) path[0] = '\0';
    return valid;
}

static OSStatus replacement_copy_matching(CFDictionaryRef query,
                                           CFTypeRef *result) {
    char helper[PATH_MAX] = {0};
    (void)resolve_verified_helper(helper, sizeof(helper));
    return copy_matching_with_helper(query, result, helper,
                                     original_copy_matching);
}

__attribute__((constructor)) static void install_production_rebinding(void) {
    struct rebinding binding = {
        .name = "SecItemCopyMatching",
        .replacement = (void *)replacement_copy_matching,
        .replaced = (void **)&original_copy_matching,
    };
    (void)rebind_symbols(&binding, 1);
}
#endif
