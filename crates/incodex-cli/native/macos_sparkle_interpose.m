/**
 * [INPUT]: 依赖匹配的宿主/协调器路径、Sparkle 初始化器与统一诊断入口。
 * [OUTPUT]: 替换重开目标；opt-in 记录匹配 updater 装配与原回调。
 * [POS]: 更新交接的进程内适配；不接管回调、block 或恢复事务。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <objc/runtime.h>
#import "macos_update_diagnostics.h"

typedef id (*UpdaterInitializer)(id, SEL, NSBundle *, NSBundle *, id, id);
/* 官方回调返回 BOOL；原始 void* 避免 ARC 复制或持有 block。 */
typedef BOOL (*SparkleWillInstallUpdateOnQuit)(id, SEL, id, id, void *);

static UpdaterInitializer originalInitializer = NULL;
static SparkleWillInstallUpdateOnQuit originalWillInstallUpdateOnQuit = NULL;
static Class observedDelegateClass = Nil;
static NSString *expectedHostPath = nil;
static NSString *coordinatorAppPath = nil;
static char matchedUpdaterAssociationKey;

static BOOL incodexWillInstallUpdateOnQuit(
    id receiver,
    SEL selector,
    id updater,
    id updateItem,
    void *immediateInstallationBlock
);

static Method directInstanceMethod(Class targetClass, SEL selector) {
    unsigned int methodCount = 0;
    Method *methods = class_copyMethodList(targetClass, &methodCount);
    Method result = NULL;
    for (unsigned int index = 0; index < methodCount; index += 1) {
        if (method_getName(methods[index]) == selector) {
            result = methods[index];
            break;
        }
    }
    free(methods);
    return result;
}

static BOOL installDelegateObserver(id delegate) {
    if (delegate == nil || !incodexUpdateDiagnosticsEnabled()) {
        return NO;
    }

    Class delegateClass = object_getClass(delegate);
    SEL selector = NSSelectorFromString(
        @"updater:willInstallUpdateOnQuit:immediateInstallationBlock:"
    );
    Method method = directInstanceMethod(delegateClass, selector);
    if (method == NULL) {
        return NO;
    }

    @synchronized (delegateClass) {
        IMP implementation = method_getImplementation(method);
        if (implementation == (IMP)incodexWillInstallUpdateOnQuit) {
            return observedDelegateClass == delegateClass;
        }
        if (observedDelegateClass != Nil && observedDelegateClass != delegateClass) {
            return NO;
        }

        originalWillInstallUpdateOnQuit = (SparkleWillInstallUpdateOnQuit)implementation;
        observedDelegateClass = delegateClass;
        method_setImplementation(method, (IMP)incodexWillInstallUpdateOnQuit);
    }
    return YES;
}

static BOOL updaterWasMatched(id updater) {
    return updater != nil &&
           objc_getAssociatedObject(updater, &matchedUpdaterAssociationKey) != nil;
}

static BOOL incodexWillInstallUpdateOnQuit(
    id receiver,
    SEL selector,
    id updater,
    id updateItem,
    void *immediateInstallationBlock
) {
    if (!updaterWasMatched(updater)) {
        return originalWillInstallUpdateOnQuit(
            receiver,
            selector,
            updater,
            updateItem,
            immediateInstallationBlock
        );
    }

    pid_t subjectPID = getpid();
    incodexUpdateDiagnostic("will-install-entered", subjectPID, 1);
    @try {
        BOOL result = originalWillInstallUpdateOnQuit(
            receiver,
            selector,
            updater,
            updateItem,
            immediateInstallationBlock
        );
        incodexUpdateDiagnostic("will-install-returned", subjectPID, result ? 1 : 0);
        return result;
    } @catch (id exception) {
        (void)exception;
        incodexUpdateDiagnostic("will-install-threw", subjectPID, -1);
        @throw;
    }
}

static NSString *canonicalPath(NSString *path) {
    return path.stringByStandardizingPath.stringByResolvingSymlinksInPath;
}

static id incodexUpdaterInitializer(
    id receiver,
    SEL selector,
    NSBundle *hostBundle,
    NSBundle *applicationBundle,
    id userDriver,
    id delegate
) {
    NSBundle *replacement = applicationBundle;
    BOOL applicationWasReplaced = NO;
    NSString *hostIdentifier = hostBundle.bundleIdentifier;
    NSString *hostPath = canonicalPath(hostBundle.bundlePath);
    NSString *applicationPath = canonicalPath(applicationBundle.bundlePath);
    BOOL matchesExpectedUpdater =
        [hostIdentifier isEqualToString:@"com.openai.codex"] &&
        [hostPath isEqualToString:expectedHostPath] &&
        [applicationPath isEqualToString:expectedHostPath];
    if (matchesExpectedUpdater) {
        pid_t subjectPID = getpid();
        incodexUpdateDiagnostic("updater-matched", subjectPID, 1);
        NSBundle *candidate = [NSBundle bundleWithPath:coordinatorAppPath];
        if (candidate.executablePath.length > 0) {
            replacement = candidate;
            applicationWasReplaced =
                ![canonicalPath(candidate.bundlePath) isEqualToString:applicationPath];
        }
    }

    id updater = originalInitializer(
        receiver,
        selector,
        hostBundle,
        replacement,
        userDriver,
        delegate
    );
    if (matchesExpectedUpdater) {
        if (updater != nil && installDelegateObserver(delegate)) {
            objc_setAssociatedObject(
                updater,
                &matchedUpdaterAssociationKey,
                @1,
                OBJC_ASSOCIATION_RETAIN_NONATOMIC
            );
        }
        if (applicationWasReplaced && updater != nil) {
            incodexUpdateDiagnostic("application-replaced", getpid(), 1);
        }
        BOOL observerReady = updaterWasMatched(updater);
        incodexUpdateDiagnostic("delegate-observer-ready", getpid(), observerReady ? 1 : 0);
    }
    return updater;
}

__attribute__((constructor)) static void installIncodexSparkleInterposer(void) {
    NSDictionary<NSString *, NSString *> *environment = NSProcessInfo.processInfo.environment;
    NSString *host = environment[@"INCODEX_MACOS_UPDATE_HOST_APP"];
    NSString *coordinator = environment[@"INCODEX_MACOS_UPDATE_COORDINATOR_APP"];
    if (host.length == 0 || coordinator.length == 0) {
        return;
    }

    Class updaterClass = NSClassFromString(@"SPUUpdater");
    SEL selector = NSSelectorFromString(@"initWithHostBundle:applicationBundle:userDriver:delegate:");
    Method method = class_getInstanceMethod(updaterClass, selector);
    if (method == NULL) {
        return;
    }

    expectedHostPath = canonicalPath(host);
    coordinatorAppPath = canonicalPath(coordinator);
    originalInitializer = (UpdaterInitializer)method_getImplementation(method);
    method_setImplementation(method, (IMP)incodexUpdaterInitializer);
    incodexUpdateDiagnostic("interposer-installed", getpid(), 1);
}
