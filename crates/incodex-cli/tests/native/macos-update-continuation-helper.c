/**
 * [INPUT]: 读取编译期固定的测试路径，并等待受控 FIFO 放行。
 * [OUTPUT]: 写入启动、结束回执，供 Rust 回归核对 PID 与父进程。
 * [POS]: 更新续接测试的登记哨兵，不调用产品 CLI 或官方应用。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#define _DARWIN_C_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

#ifndef INCODEX_TEST_HELPER_LOG
#error "test-owned helper log path is required"
#endif
#ifndef INCODEX_TEST_HELPER_MARKER
#error "test-owned helper marker path is required"
#endif
#ifndef INCODEX_TEST_HELPER_GATE
#error "test-owned helper gate path is required"
#endif
#ifndef INCODEX_TEST_HELPER_END
#error "test-owned helper end path is required"
#endif

static int write_record(const char *path, const char *record, size_t length, int flags) {
    int descriptor = open(path, flags | O_CLOEXEC, 0600);
    if (descriptor < 0) return -1;
    ssize_t written = write(descriptor, record, length);
    int result = written == (ssize_t)length ? 0 : -1;
    close(descriptor);
    return result;
}

int main(void) {
    const char *log = INCODEX_TEST_HELPER_LOG;
    const char *marker = INCODEX_TEST_HELPER_MARKER;
    const char *gate = INCODEX_TEST_HELPER_GATE;
    const char *end = INCODEX_TEST_HELPER_END;
    char record[160];
    int length = snprintf(record, sizeof(record), "{\"pid\":%d,\"ppid\":%d}\n",
                          getpid(), getppid());
    if (length <= 0 || (size_t)length >= sizeof(record)) return 21;
    if (write_record(log, record, (size_t)length, O_WRONLY | O_APPEND | O_CREAT) != 0) return 22;
    if (write_record(marker, record, (size_t)length, O_WRONLY | O_CREAT | O_EXCL) != 0) return 23;

    int descriptor = open(gate, O_RDONLY | O_CLOEXEC);
    if (descriptor < 0) return 24;
    char token[3];
    ssize_t received;
    do {
        received = read(descriptor, token, sizeof(token));
    } while (received < 0 && errno == EINTR);
    close(descriptor);
    if (received != (ssize_t)sizeof(token) || memcmp(token, "GO\n", sizeof(token)) != 0) {
        return 25;
    }

    length = snprintf(record, sizeof(record),
                      "{\"pid\":%d,\"ppid\":%d,\"exitCode\":0}\n", getpid(), getppid());
    if (length <= 0 || (size_t)length >= sizeof(record)) return 26;
    if (write_record(end, record, (size_t)length, O_WRONLY | O_CREAT | O_EXCL) != 0) return 27;
    return 0;
}
