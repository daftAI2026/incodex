#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <signal.h>
#include <spawn.h>
#include <stdbool.h>
#include <stddef.h>
#include <string.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#include "fishhook.h"

static const size_t kMaximumHelperOutput = 4096;
static const int64_t kHelperTimeoutMilliseconds = 2000;
typedef OSStatus (*CopyMatching)(CFDictionaryRef, CFTypeRef *);

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

    if (!WIFEXITED(child_status) || WEXITSTATUS(child_status) != 0 ||
        used == 0) {
        memset(output, 0, capacity);
        return 70;
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
        return errSecAuthFailed;
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
#endif
