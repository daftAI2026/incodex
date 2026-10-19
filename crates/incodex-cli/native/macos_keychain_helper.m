#include <CoreFoundation/CoreFoundation.h>
#include <LocalAuthentication/LocalAuthentication.h>
#include <Security/Security.h>
#include <errno.h>
#include <fcntl.h>
#include <libproc.h>
#include <limits.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <sys/proc_info.h>
#include <unistd.h>

static const char kExpectedParent[] =
    "/Applications/ChatGPT.app/Contents/MacOS/ChatGPT";
static const char kExpectedIdentifier[] = "com.openai.codex";
static const char kService[] = "Codex Storage Key";
static const char kAccount[] = "Codex";
static const size_t kMaximumOutputBytes = 4096;
static volatile sig_atomic_t gCancellationWriteFd = -1;

typedef struct {
    CFDictionaryRef query;
    CFTypeRef result;
    OSStatus status;
    int completion_write_fd;
} MatchingWorker;

static void request_cancellation(int signal_number) {
    (void)signal_number;
    int saved_errno = errno;
    int fd = (int)gCancellationWriteFd;
    if (fd >= 0) {
        const unsigned char byte = 1;
        (void)write(fd, &byte, sizeof(byte));
    }
    errno = saved_errno;
}

static void close_pipe(int pipe_fds[2]) {
    if (pipe_fds[0] >= 0) close(pipe_fds[0]);
    if (pipe_fds[1] >= 0) close(pipe_fds[1]);
    pipe_fds[0] = -1;
    pipe_fds[1] = -1;
}

static void *copy_matching_worker(void *raw_worker) {
    @autoreleasepool {
        MatchingWorker *worker = raw_worker;
        worker->status = SecItemCopyMatching(worker->query, &worker->result);
        const unsigned char byte = 1;
        while (write(worker->completion_write_fd, &byte, sizeof(byte)) < 0 &&
               errno == EINTR) {
        }
    }
    return NULL;
}

static OSStatus copy_matching_cancellable(CFDictionaryRef query,
                                          LAContext *context,
                                          CFTypeRef *result) {
    int cancellation_pipe[2] = {-1, -1};
    int completion_pipe[2] = {-1, -1};
    if (pipe(cancellation_pipe) != 0 || pipe(completion_pipe) != 0) {
        close_pipe(cancellation_pipe);
        close_pipe(completion_pipe);
        return errSecAllocate;
    }
    int cancellation_flags = fcntl(cancellation_pipe[1], F_GETFL, 0);
    if (cancellation_flags < 0 ||
        fcntl(cancellation_pipe[1], F_SETFL,
              cancellation_flags | O_NONBLOCK) != 0) {
        close_pipe(cancellation_pipe);
        close_pipe(completion_pipe);
        return errSecInternalComponent;
    }

    struct sigaction action = {0};
    struct sigaction previous_action = {0};
    action.sa_handler = request_cancellation;
    sigemptyset(&action.sa_mask);
    gCancellationWriteFd = cancellation_pipe[1];
    if (sigaction(SIGTERM, &action, &previous_action) != 0) {
        gCancellationWriteFd = -1;
        close_pipe(cancellation_pipe);
        close_pipe(completion_pipe);
        return errSecInternalComponent;
    }

    MatchingWorker worker = {
        .query = query,
        .result = NULL,
        .status = errSecInternalComponent,
        .completion_write_fd = completion_pipe[1],
    };
    pthread_t thread;
    if (pthread_create(&thread, NULL, copy_matching_worker, &worker) != 0) {
        gCancellationWriteFd = -1;
        (void)sigaction(SIGTERM, &previous_action, NULL);
        close_pipe(cancellation_pipe);
        close_pipe(completion_pipe);
        return errSecInternalComponent;
    }

    bool cancelled = false;
    for (;;) {
        struct pollfd descriptors[] = {
            {.fd = completion_pipe[0], .events = POLLIN | POLLHUP, .revents = 0},
            {.fd = cancellation_pipe[0], .events = POLLIN | POLLHUP, .revents = 0},
        };
        int poll_status = poll(descriptors, 2, -1);
        if (poll_status < 0 && errno == EINTR) continue;
        if (poll_status < 0) {
            cancelled = true;
            [context invalidate];
            break;
        }
        if (descriptors[1].revents & (POLLIN | POLLHUP)) {
            cancelled = true;
            [context invalidate];
            break;
        }
        if (descriptors[0].revents & (POLLIN | POLLHUP)) break;
    }

    (void)pthread_join(thread, NULL);
    gCancellationWriteFd = -1;
    (void)sigaction(SIGTERM, &previous_action, NULL);
    close_pipe(cancellation_pipe);
    close_pipe(completion_pipe);

    if (cancelled) {
        if (worker.result) CFRelease(worker.result);
        *result = NULL;
        return errSecUserCanceled;
    }
    *result = worker.result;
    return worker.status;
}

static bool parent_has_expected_identity(pid_t parent_pid) {
    if (getuid() != geteuid() || getgid() != getegid()) return false;

    struct proc_bsdinfo info = {0};
    if (proc_pidinfo(parent_pid, PROC_PIDTBSDINFO, 0, &info, sizeof(info)) !=
        sizeof(info)) {
        return false;
    }
    if (info.pbi_ruid != getuid() || info.pbi_uid != geteuid()) return false;

    char observed_path[PROC_PIDPATHINFO_MAXSIZE] = {0};
    if (proc_pidpath(parent_pid, observed_path, sizeof(observed_path)) <= 0) {
        return false;
    }
    char expected_path[PATH_MAX] = {0};
    char canonical_observed[PATH_MAX] = {0};
    if (!realpath(kExpectedParent, expected_path) ||
        !realpath(observed_path, canonical_observed) ||
        strcmp(expected_path, canonical_observed) != 0) {
        return false;
    }

    int parent = parent_pid;
    CFNumberRef pid_number = CFNumberCreate(
        kCFAllocatorDefault, kCFNumberIntType, &parent);
    if (!pid_number) return false;
    const void *keys[] = {kSecGuestAttributePid};
    const void *values[] = {pid_number};
    CFDictionaryRef attributes = CFDictionaryCreate(
        kCFAllocatorDefault, keys, values, 1, &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);
    CFRelease(pid_number);
    if (!attributes) return false;

    SecCodeRef code = NULL;
    OSStatus status = SecCodeCopyGuestWithAttributes(
        NULL, attributes, kSecCSDefaultFlags, &code);
    CFRelease(attributes);
    if (status != errSecSuccess || !code) return false;

    CFDictionaryRef signing = NULL;
    status = SecCodeCopySigningInformation(
        code, kSecCSSigningInformation, &signing);
    CFRelease(code);
    if (status != errSecSuccess || !signing) return false;

    CFTypeRef identifier = CFDictionaryGetValue(signing, kSecCodeInfoIdentifier);
    CFStringRef expected_identifier = CFStringCreateWithCString(
        kCFAllocatorDefault, kExpectedIdentifier, kCFStringEncodingUTF8);
    bool matches = expected_identifier && identifier &&
        CFGetTypeID(identifier) == CFStringGetTypeID() &&
        CFStringCompare((CFStringRef)identifier, expected_identifier, 0) ==
            kCFCompareEqualTo;
    if (expected_identifier) CFRelease(expected_identifier);
    CFRelease(signing);
    return matches;
}

static int write_all(const unsigned char *bytes, size_t length) {
    size_t offset = 0;
    while (offset < length) {
        ssize_t written = write(STDOUT_FILENO, bytes + offset, length - offset);
        if (written < 0 && errno == EINTR) continue;
        if (written <= 0) return 74;
        offset += (size_t)written;
    }
    return 0;
}

static void secure_zero(unsigned char *bytes, size_t length) {
    volatile unsigned char *cursor = bytes;
    while (length-- > 0) *cursor++ = 0;
}

static int run(int argc, char **argv) {
    bool authorize_only = argc == 2 && strcmp(argv[1], "--authorize") == 0;
    if (!authorize_only) {
        if (argc != 1) return 64;
        pid_t parent_pid = getppid();
        if (parent_pid <= 1 || !parent_has_expected_identity(parent_pid)) {
            return 69;
        }
    }

    CFStringRef service = CFStringCreateWithCString(
        kCFAllocatorDefault, kService, kCFStringEncodingUTF8);
    CFStringRef account = CFStringCreateWithCString(
        kCFAllocatorDefault, kAccount, kCFStringEncodingUTF8);
    if (!service || !account) {
        if (service) CFRelease(service);
        if (account) CFRelease(account);
        return 65;
    }
    CFMutableDictionaryRef query = CFDictionaryCreateMutable(
        kCFAllocatorDefault, 5, &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);
    if (!query) {
        CFRelease(service);
        CFRelease(account);
        return 65;
    }
    CFDictionarySetValue(query, kSecClass, kSecClassGenericPassword);
    CFDictionarySetValue(query, kSecAttrService, service);
    CFDictionarySetValue(query, kSecAttrAccount, account);
    CFDictionarySetValue(query, kSecReturnData, kCFBooleanTrue);
    CFDictionarySetValue(query, kSecMatchLimit, kSecMatchLimitOne);
    LAContext *authentication_context = nil;
    if (authorize_only) {
        authentication_context = [[LAContext alloc] init];
        CFDictionarySetValue(query, kSecUseAuthenticationContext,
                             (__bridge const void *)authentication_context);
    } else {
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
        // LAContext.interactionNotAllowed can still block inside securityd on
        // current macOS. The legacy fail value preserves the required
        // immediate, noninteractive runtime contract.
        CFDictionarySetValue(query, kSecUseAuthenticationUI,
                             kSecUseAuthenticationUIFail);
#pragma clang diagnostic pop
    }
    CFRelease(service);
    CFRelease(account);

    CFTypeRef result = NULL;
    OSStatus status = authorize_only
        ? copy_matching_cancellable(query, authentication_context, &result)
        : SecItemCopyMatching(query, &result);
    CFRelease(query);
    if (status != errSecSuccess || !result ||
        CFGetTypeID(result) != CFDataGetTypeID()) {
        if (result) CFRelease(result);
        return status == errSecItemNotFound ? 44 : 68;
    }

    CFDataRef data = (CFDataRef)result;
    CFIndex length = CFDataGetLength(data);
    if (length <= 0 || (size_t)length > kMaximumOutputBytes) {
        CFRelease(data);
        return 68;
    }
    if (authorize_only) {
        CFRelease(data);
        return 0;
    }
    unsigned char bytes[4096] = {0};
    CFDataGetBytes(data, CFRangeMake(0, length), bytes);
    CFRelease(data);
    int write_result = write_all(bytes, (size_t)length);
    secure_zero(bytes, sizeof(bytes));
    return write_result;
}

int main(int argc, char **argv) {
    @autoreleasepool {
        return run(argc, argv);
    }
}
