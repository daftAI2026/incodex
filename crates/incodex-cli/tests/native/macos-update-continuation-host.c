/**
 * [INPUT]: 接收编译期固定的本次测试目录标记路径。
 * [OUTPUT]: 写入自身 PID 后等待测试进程发送正常终止信号。
 * [POS]: 续接测试的假宿主重开探针，不读取继承环境变量。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#define _DARWIN_C_SOURCE
#include <fcntl.h>
#include <stdio.h>
#include <unistd.h>

#ifndef INCODEX_TEST_RELAUNCH_MARKER
#error "INCODEX_TEST_RELAUNCH_MARKER must be a test-owned absolute path"
#endif

int main(void) {
    int descriptor = open(INCODEX_TEST_RELAUNCH_MARKER,
                          O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (descriptor < 0) return 31;
    char record[80];
    int length = snprintf(record, sizeof(record), "{\"pid\":%d}\n", getpid());
    if (length <= 0 || (size_t)length >= sizeof(record)) {
        close(descriptor);
        return 32;
    }
    ssize_t written = write(descriptor, record, (size_t)length);
    close(descriptor);
    if (written != length) return 33;
    for (;;) pause();
}
