/**
 * [INPUT]: 依赖固定事件码、进程环境与 macOS os_log。
 * [OUTPUT]: 提供唯一的 opt-in 更新诊断日志入口。
 * [POS]: native 共用边界；测试构建把相同字段写入 stderr。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#ifndef INCODEX_MACOS_UPDATE_DIAGNOSTICS_H
#define INCODEX_MACOS_UPDATE_DIAGNOSTICS_H

#include <dispatch/dispatch.h>
#include <os/log.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <unistd.h>

static inline bool incodexUpdateDiagnosticsEnabled(void) {
    const char *value = getenv("INCODEX_DIAGNOSTIC_UPDATE_TRACE");
    return value != NULL && value[0] == '1' && value[1] == '\0';
}

static inline void incodexUpdateDiagnostic(const char *code, pid_t subjectPID, int value) {
    if (!incodexUpdateDiagnosticsEnabled()) {
        return;
    }

    const int ownPID = (int)getpid();
#if defined(INCODEX_DIAGNOSTIC_TRACE_TEST_STDERR)
    fprintf(stderr,
            "[incodex-update-diagnostic] code=%s pid=%d subject_pid=%d value=%d\n",
            code,
            ownPID,
            (int)subjectPID,
            value);
#else
    static os_log_t logger = NULL;
    static dispatch_once_t loggerOnce;
    dispatch_once(&loggerOnce, ^{
        logger = os_log_create("com.daftai.incodex", "macos-update-diagnostic");
    });
    os_log_with_type(logger,
                     OS_LOG_TYPE_INFO,
                     "[incodex-update-diagnostic] code=%{public}s pid=%d subject_pid=%d value=%d",
                     code,
                     ownPID,
                     (int)subjectPID,
                     value);
#endif
}

#endif
