/**
 * [INPUT]: 依赖AppKit、Foundation、共用pending协议与登记helper。
 * [OUTPUT]: 协调退出与更新恢复；活宿主用退出事件续接，busy回执限定重试，正常关闭不重开。
 * [POS]: Sparkle的替代重开目标；复用同二进制，不增加常驻服务。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <AppKit/AppKit.h>
#import <CommonCrypto/CommonDigest.h>
#import <Foundation/Foundation.h>
#import <dispatch/dispatch.h>
#import <fcntl.h>
#import <libproc.h>
#import <signal.h>
#import <sys/file.h>
#import <sys/stat.h>
#import <unistd.h>
#import <sysexits.h>
#import "macos_update_diagnostics.h"
#import "macos_update_pending.h"
#import <limits.h>
#import <stdlib.h>
#import <string.h>

static const uint64_t QueuedInstallWaitSeconds = 180;
typedef NS_ENUM(NSUInteger, IncodexContinuationPhase) {
    IncodexContinuationWaitingParent,
    IncodexContinuationWaitingBuild,
    IncodexContinuationRecovering,
};

@interface IncodexUpdateDelegate : NSObject <NSApplicationDelegate>
@property(nonatomic) BOOL initialLaunch;
@property(nonatomic) BOOL consoleContinuation;
@property(nonatomic) IncodexContinuationPhase continuationPhase;
@property(nonatomic, copy) NSDictionary *continuationPending;
@property(nonatomic, strong) dispatch_source_t parentExitSource;
@property(nonatomic, strong) dispatch_source_t buildChangeSource;
@property(nonatomic, strong) dispatch_source_t installWaitTimer;
@property(nonatomic) pid_t hostPID;
@property(nonatomic, copy) NSString *installID;
@property(nonatomic, copy) NSString *appPath;
@property(nonatomic, copy) NSString *helperPath;
@property(nonatomic, copy) NSString *helperSha256;
@property(nonatomic, copy) NSString *handoffId;
@property(nonatomic, copy) NSString *readyPath;
@property(nonatomic, copy) NSString *pendingPath;
@property(nonatomic, copy) NSString *sourceBuild;
@property(nonatomic) BOOL terminationPending;
@property(nonatomic) BOOL allowTermination;
@property(nonatomic) dispatch_source_t hostExitSource;
@property(nonatomic, copy) NSArray *recoveryExitSources;
@end

@implementation IncodexUpdateDelegate

- (BOOL)processIsRunning:(pid_t)pid {
    return pid > 0 && (kill(pid, 0) == 0 || errno == EPERM);
}

- (BOOL)coordinatorProcessOwnsPending:(pid_t)pid {
    if (![self processIsRunning:pid]) return NO;

    char executable[PROC_PIDPATHINFO_MAXSIZE] = {0};
    if (proc_pidpath(pid, executable, sizeof(executable)) <= 0) return NO;
    NSString *ownerPath = [NSString stringWithUTF8String:executable];
    NSString *coordinatorPath = NSBundle.mainBundle.executablePath;
    if (ownerPath.length == 0 || coordinatorPath.length == 0) return NO;
    ownerPath = ownerPath.stringByStandardizingPath.stringByResolvingSymlinksInPath;
    coordinatorPath = coordinatorPath.stringByStandardizingPath.stringByResolvingSymlinksInPath;
    return [ownerPath isEqualToString:coordinatorPath];
}

- (BOOL)writeJSON:(NSDictionary *)body toPath:(NSString *)path {
    return incodexUpdateWritePrivateJSON(body, path);
}

- (void)stopCoordinator {
    [self cancelRecoveryExitObservation];
    if (self.consoleContinuation) exit(0);
    [NSApp terminate:nil];
}

- (void)writeReady {
    NSDictionary *ready = @{ @"schemaVersion": @1, @"pid": @(NSProcessInfo.processInfo.processIdentifier) };
    if (![self writeJSON:ready toPath:self.readyPath]) {
        [self stopCoordinator];
    }
}

- (BOOL)persistPending {
    __block BOOL persisted = NO;
    [self withPendingLock:^BOOL{
        NSDictionary *current = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
        NSNumber *owner = current[@"coordinatorPid"];
        NSString *handoffId = current[@"handoffId"];
        if ([handoffId isKindOfClass:NSString.class] &&
            ![handoffId isEqual:self.handoffId] && [owner isKindOfClass:NSNumber.class] &&
            [self coordinatorProcessOwnsPending:owner.intValue]) return NO;
        persisted = [self writeJSON:[self pendingSnapshot] toPath:self.pendingPath];
        return persisted;
    }];
    return persisted;
}

- (NSDictionary *)pendingSnapshot {
    return @{
        @"schemaVersion": @2,
        @"installId": self.installID,
        @"appPath": self.appPath,
        @"helperPath": self.helperPath,
        @"helperSha256": self.helperSha256,
        @"handoffId": self.handoffId,
        @"coordinatorPid": @(NSProcessInfo.processInfo.processIdentifier),
        @"sourceBuild": self.sourceBuild ?: @"",
    };
}

- (NSString *)currentHostBuild {
    NSString *infoPath = [self.appPath stringByAppendingPathComponent:@"Contents/Info.plist"];
    NSDictionary *info = [NSDictionary dictionaryWithContentsOfFile:infoPath];
    id build = info[@"CFBundleVersion"];
    if ([build isKindOfClass:NSString.class]) return build;
    if ([build isKindOfClass:NSNumber.class]) return [build stringValue];
    return nil;
}

- (BOOL)hostBundleChanged {
    NSString *currentBuild = [self currentHostBuild];
    return self.sourceBuild.length > 0 && currentBuild.length > 0 &&
           ![currentBuild isEqualToString:self.sourceBuild];
}

- (void)armHostExitObservation {
    if (![self processIsRunning:self.hostPID]) return;
    self.hostExitSource = dispatch_source_create(
        DISPATCH_SOURCE_TYPE_PROC,
        (uintptr_t)self.hostPID,
        DISPATCH_PROC_EXIT,
        dispatch_get_main_queue()
    );
    __weak typeof(self) weakSelf = self;
    dispatch_source_set_event_handler(self.hostExitSource, ^{
        typeof(self) self = weakSelf;
        if (self == nil) return;
        if (self.terminationPending) {
            incodexUpdateDiagnostic("host-exit-termination-pending", self.hostPID, YES);
            [NSApp replyToApplicationShouldTerminate:YES];
            return;
        }
        if ([self hostBundleChanged]) {
            incodexUpdateDiagnostic("host-exit-build-changed", self.hostPID, YES);
            [self recoverAfterUpdate:[self pendingSnapshot] relaunchHost:NO];
            return;
        }
        if ([self continueQueuedUpdateAfterOrdinaryExit]) return;
        incodexUpdateDiagnostic("host-exit-ordinary-cleanup", self.hostPID, NO);
        [self removePendingIfOwned:[self pendingSnapshot]];
        self.allowTermination = YES;
        [self stopCoordinator];
    });
    dispatch_resume(self.hostExitSource);
}

// ---------- 普通退出的单次交接 ----------
- (BOOL)continueQueuedUpdateAfterOrdinaryExit {
    __block BOOL queued = NO;
    __block BOOL launched = NO;
    __block pid_t continuationPID = 0;
    [self withPendingLock:^BOOL{
        NSDictionary *current = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
        if (![self pending:current isOwnedBy:[self pendingSnapshot]] ||
            ![current[@"installQueued"] isEqual:@YES]) return NO;
        queued = YES;
        NSMutableDictionary *next = current.mutableCopy;
        next[@"sourceBuild"] = self.sourceBuild;
        next[@"relaunchHost"] = @NO;
        if (![self writeJSON:next toPath:self.pendingPath]) return NO;
        NSTask *task = [NSTask new];
        task.executableURL = NSBundle.mainBundle.executableURL;
        task.arguments = @[@"--continue-update", [NSString stringWithFormat:@"%d", getpid()]];
        NSMutableDictionary *environment = NSProcessInfo.processInfo.environment.mutableCopy;
        [environment removeObjectForKey:@"INCODEX_MACOS_UPDATE_COORDINATOR"];
        task.environment = environment;
        launched = [task launchAndReturnError:nil];
        if (launched) continuationPID = task.processIdentifier;
        return launched;
    }];
    if (!queued) return NO;
    incodexUpdateDiagnostic("continuation-launch-result", continuationPID, launched);
    // Sparkle等待旧AppKit实例退出。启动失败也不阻塞官方换包。
    self.allowTermination = YES;
    [self stopCoordinator];
    return YES;
}

- (void)checkQueuedReplacement:(NSDictionary *)pending {
    if (self.continuationPhase != IncodexContinuationWaitingBuild) return;
    if (![self pendingIsOwned:pending]) {
        [self stopCoordinator];
        return;
    }
    if (![self hostBundleChanged]) return;
    self.continuationPhase = IncodexContinuationRecovering;
    incodexUpdateDiagnostic("continuation-build-changed", 0, YES);
    if (self.buildChangeSource != nil) dispatch_source_cancel(self.buildChangeSource);
    if (self.installWaitTimer != nil) dispatch_source_cancel(self.installWaitTimer);
    [self recoverAfterUpdate:pending relaunchHost:NO];
}

- (void)adoptQueuedContinuation {
    if (self.continuationPhase != IncodexContinuationWaitingParent) return;
    self.continuationPhase = IncodexContinuationWaitingBuild;
    if (self.parentExitSource != nil) {
        dispatch_source_cancel(self.parentExitSource);
        self.parentExitSource = nil;
    }
    NSDictionary *pending = [self loadAndAdoptPendingMatching:self.continuationPending];
    if (pending == nil || ![pending[@"installQueued"] isEqual:@YES] ||
        ![pending[@"sourceBuild"] isKindOfClass:NSString.class] ||
        [pending[@"sourceBuild"] length] == 0) {
        [self stopCoordinator];
        return;
    }
    self.appPath = pending[@"appPath"];
    self.sourceBuild = pending[@"sourceBuild"];
    incodexUpdateDiagnostic("continuation-pending-adopted", 0, YES);
    NSString *parent = self.appPath.stringByDeletingLastPathComponent;
    int descriptor = open(parent.fileSystemRepresentation, O_EVTONLY | O_CLOEXEC);
    if (descriptor < 0) {
        [self stopCoordinator];
        return;
    }
    self.buildChangeSource = dispatch_source_create(DISPATCH_SOURCE_TYPE_VNODE,
        (uintptr_t)descriptor, DISPATCH_VNODE_WRITE | DISPATCH_VNODE_RENAME | DISPATCH_VNODE_DELETE,
        dispatch_get_main_queue());
    if (self.buildChangeSource == nil) {
        close(descriptor);
        [self stopCoordinator];
        return;
    }
    dispatch_source_set_cancel_handler(self.buildChangeSource, ^{ close(descriptor); });
    dispatch_source_set_event_handler(self.buildChangeSource, ^{
        [self checkQueuedReplacement:pending];
    });
    dispatch_resume(self.buildChangeSource);
    // 先观察再读取，覆盖Sparkle在watch建立前完成rename的时序。
    [self checkQueuedReplacement:pending];
}

- (void)runContinuationAfterParent:(pid_t)parentPID {
    self.consoleContinuation = YES;
    self.pendingPath = incodexUpdatePendingPathForCoordinator(NSBundle.mainBundle.bundlePath);
    self.continuationPending = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
    if (![self.continuationPending[@"coordinatorPid"] isEqual:@(parentPID)] ||
        ![self.continuationPending[@"installQueued"] isEqual:@YES]) {
        [self stopCoordinator];
        return;
    }
    self.installWaitTimer = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0,
        dispatch_get_main_queue());
    dispatch_source_set_timer(self.installWaitTimer,
        dispatch_time(DISPATCH_TIME_NOW, QueuedInstallWaitSeconds * NSEC_PER_SEC),
        DISPATCH_TIME_FOREVER, NSEC_PER_SEC);
    dispatch_source_set_event_handler(self.installWaitTimer, ^{
        if (self.continuationPhase == IncodexContinuationRecovering) return;
        incodexUpdateDiagnostic("continuation-wait-expired", 0, NO);
        [self stopCoordinator];
    });
    dispatch_resume(self.installWaitTimer);
    if (![self coordinatorProcessOwnsPending:parentPID]) {
        [self adoptQueuedContinuation];
        return;
    }
    self.parentExitSource = dispatch_source_create(DISPATCH_SOURCE_TYPE_PROC,
        (uintptr_t)parentPID, DISPATCH_PROC_EXIT, dispatch_get_main_queue());
    if (self.parentExitSource == nil) {
        [self stopCoordinator];
        return;
    }
    dispatch_source_set_event_handler(self.parentExitSource, ^{ [self adoptQueuedContinuation]; });
    dispatch_resume(self.parentExitSource);
    if (![self coordinatorProcessOwnsPending:parentPID]) [self adoptQueuedContinuation];
}

- (BOOL)requestHostTermination {
    NSRunningApplication *host =
        [NSRunningApplication runningApplicationWithProcessIdentifier:self.hostPID];
    return host != nil && [host terminate];
}

- (NSApplicationTerminateReply)applicationShouldTerminate:(NSApplication *)sender {
    if (!self.initialLaunch || self.allowTermination) {
        return NSTerminateNow;
    }
    if (![self processIsRunning:self.hostPID]) {
        return NSTerminateNow;
    }
    if (![self requestHostTermination]) {
        return [self processIsRunning:self.hostPID] ? NSTerminateCancel : NSTerminateNow;
    }
    self.terminationPending = YES;
    return NSTerminateLater;
}

- (void)launchHostAndExit {
    NSURL *applicationURL = [NSURL fileURLWithPath:self.appPath];
    NSWorkspaceOpenConfiguration *configuration = [NSWorkspaceOpenConfiguration configuration];
    [NSWorkspace.sharedWorkspace openApplicationAtURL:applicationURL
                                         configuration:configuration
                                     completionHandler:^(__unused NSRunningApplication *application, __unused NSError *error) {
        [self stopCoordinator];
    }];
}

- (NSDictionary *)readPrivateJSONAtPath:(NSString *)path maxBytes:(off_t)maxBytes {
    return incodexUpdateReadPrivateJSON(path, maxBytes);
}

- (BOOL)withPendingLock:(BOOL (^)(void))body {
    return incodexUpdateWithPendingLock(self.pendingPath, body);
}

- (BOOL)isSHA256:(NSString *)value {
    if (![value isKindOfClass:NSString.class] || value.length != 64) return NO;
    NSCharacterSet *hex = [NSCharacterSet characterSetWithCharactersInString:@"0123456789abcdef"];
    return [value rangeOfCharacterFromSet:hex.invertedSet].location == NSNotFound;
}

- (NSString *)sha256ForDescriptor:(int)descriptor size:(off_t)size {
    if (size <= 0 || size > 16 * 1024 * 1024 || lseek(descriptor, 0, SEEK_SET) < 0) return nil;
    CC_SHA256_CTX context;
    CC_SHA256_Init(&context);
    unsigned char buffer[64 * 1024];
    off_t remaining = size;
    while (remaining > 0) {
        ssize_t count = read(descriptor, buffer, MIN((off_t)sizeof(buffer), remaining));
        if (count <= 0) return nil;
        CC_SHA256_Update(&context, buffer, (CC_LONG)count);
        remaining -= count;
    }
    unsigned char digest[CC_SHA256_DIGEST_LENGTH];
    CC_SHA256_Final(digest, &context);
    NSMutableString *hex = [NSMutableString stringWithCapacity:CC_SHA256_DIGEST_LENGTH * 2];
    for (NSUInteger index = 0; index < CC_SHA256_DIGEST_LENGTH; index += 1) {
        [hex appendFormat:@"%02x", digest[index]];
    }
    return hex;
}

- (BOOL)verifyHelperAtPath:(NSString *)helper sha256:(NSString *)sha256 {
    if (![self isSHA256:sha256] || !helper.isAbsolutePath) return NO;
    NSString *root = self.pendingPath.stringByDeletingLastPathComponent.stringByDeletingLastPathComponent;
    NSString *expected = [[[[root stringByAppendingPathComponent:@"helpers/macos-update"]
        stringByAppendingPathComponent:sha256] stringByAppendingPathComponent:@"incodex"]
        stringByStandardizingPath];
    if (![[helper stringByStandardizingPath] isEqualToString:expected]) return NO;

    int descriptor = open(helper.fileSystemRepresentation, O_RDONLY | O_NOFOLLOW);
    if (descriptor < 0) return NO;
    struct stat metadata;
    BOOL valid = fstat(descriptor, &metadata) == 0 && S_ISREG(metadata.st_mode) &&
                 metadata.st_uid == geteuid() && (metadata.st_mode & 0777) == 0700;
    NSString *actual = valid ? [self sha256ForDescriptor:descriptor size:metadata.st_size] : nil;
    close(descriptor);
    return actual != nil && [actual isEqualToString:sha256];
}

- (NSDictionary *)currentRegistrationForPending:(NSDictionary *)pending {
    NSString *directory = self.pendingPath.stringByDeletingLastPathComponent;
    NSString *registrationPath = [directory stringByAppendingPathComponent:@"registration.json"];
    NSDictionary *registration = [self readPrivateJSONAtPath:registrationPath maxBytes:64 * 1024];
    if (![registration isKindOfClass:NSDictionary.class] ||
        ![registration[@"schemaVersion"] isEqual:@2] ||
        ![registration[@"installId"] isEqual:pending[@"installId"]] ||
        ![registration[@"appPath"] isEqual:pending[@"appPath"]]) {
        return nil;
    }
    NSString *helper = registration[@"helperPath"];
    NSString *sha256 = registration[@"helperSha256"];
    if (![helper isKindOfClass:NSString.class] || ![sha256 isKindOfClass:NSString.class] ||
        ![self verifyHelperAtPath:helper sha256:sha256]) return nil;
    return registration;
}

- (BOOL)removePendingIfOwned:(NSDictionary *)expected {
    __block BOOL removed = NO;
    [self withPendingLock:^BOOL{
        NSDictionary *current = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
        if (![self pending:current isOwnedBy:expected]) return NO;
        removed = [NSFileManager.defaultManager removeItemAtPath:self.pendingPath error:nil];
        return removed;
    }];
    incodexUpdateDiagnostic("pending-remove-result", self.hostPID, removed);
    return removed;
}

- (BOOL)pending:(NSDictionary *)current isOwnedBy:(NSDictionary *)expected {
    if (current == nil || ![current[@"installId"] isEqual:expected[@"installId"]] ||
        ![current[@"appPath"] isEqual:expected[@"appPath"]]) return NO;
    NSString *handoffId = expected[@"handoffId"];
    if (![handoffId isKindOfClass:NSString.class] || handoffId.length == 0 ||
        ![current[@"handoffId"] isEqual:handoffId]) return NO;
    NSNumber *owner = current[@"coordinatorPid"];
    return [owner isKindOfClass:NSNumber.class] &&
           owner.intValue == NSProcessInfo.processInfo.processIdentifier;
}

- (BOOL)pendingIsOwned:(NSDictionary *)expected {
    __block BOOL owned = NO;
    [self withPendingLock:^BOOL{
        NSDictionary *current = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
        owned = [self pending:current isOwnedBy:expected];
        return YES;
    }];
    return owned;
}

- (NSDictionary *)loadAndAdoptPending {
    return [self loadAndAdoptPendingMatching:nil];
}

- (NSDictionary *)loadAndAdoptPendingMatching:(NSDictionary *)expected {
    __block NSDictionary *adopted = nil;
    [self withPendingLock:^BOOL{
        NSDictionary *current = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
        if (current == nil || ![current[@"installId"] isKindOfClass:NSString.class] ||
            ![current[@"appPath"] isKindOfClass:NSString.class]) return NO;
        if (expected != nil && !incodexUpdateSamePendingIdentity(current, expected)) return NO;
        NSNumber *owner = current[@"coordinatorPid"];
        if ([owner isKindOfClass:NSNumber.class] &&
            owner.intValue != NSProcessInfo.processInfo.processIdentifier &&
            [self coordinatorProcessOwnsPending:owner.intValue]) return NO;
        NSMutableDictionary *next = current.mutableCopy;
        NSString *handoffId = next[@"handoffId"];
        if (![handoffId isKindOfClass:NSString.class] || handoffId.length == 0) {
            next[@"handoffId"] = NSUUID.UUID.UUIDString.lowercaseString;
        }
        next[@"schemaVersion"] = @2;
        next[@"coordinatorPid"] = @(NSProcessInfo.processInfo.processIdentifier);
        if (![self writeJSON:next toPath:self.pendingPath]) return NO;
        adopted = next.copy;
        return YES;
    }];
    return adopted;
}

- (void)finishRecovery:(NSDictionary *)pending relaunchHost:(BOOL)relaunchHost success:(BOOL)success {
    if (success) {
        if (![self removePendingIfOwned:pending]) {
            [self stopCoordinator];
            return;
        }
    } else if (![self pendingIsOwned:pending]) {
        [self stopCoordinator];
        return;
    }
    incodexUpdateDiagnostic("relaunch-host", self.hostPID, relaunchHost);
    if (relaunchHost && success && self.appPath.length > 0) {
        [self launchHostAndExit];
    } else {
        self.allowTermination = YES;
        [self stopCoordinator];
    }
}

- (BOOL)launchTaskIfPendingOwned:(NSDictionary *)pending task:(NSTask *)task error:(NSError **)error {
    __block BOOL launched = NO;
    [self withPendingLock:^BOOL{
        NSDictionary *current = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
        if (![self pending:current isOwnedBy:pending]) return NO;
        launched = [task launchAndReturnError:error];
        return launched;
    }];
    pid_t helperPID = launched ? task.processIdentifier : 0;
    incodexUpdateDiagnostic("helper-launch-result", helperPID, launched);
    return launched;
}

// ---- 新宿主已先打开：只等正常退出，不接管它的生命周期 ----
- (NSArray<NSNumber *> *)runningHostPIDs {
    NSDictionary *info = [NSDictionary dictionaryWithContentsOfFile:
        [self.appPath stringByAppendingPathComponent:@"Contents/Info.plist"]];
    NSString *name = info[@"CFBundleExecutable"];
    if (![name isKindOfClass:NSString.class] || name.length == 0 ||
        name.pathComponents.count != 1 || [name isEqualToString:@"."] ||
        [name isEqualToString:@".."]) return nil;
    NSString *expected = [[[self.appPath stringByAppendingPathComponent:@"Contents/MacOS"]
        stringByAppendingPathComponent:name] stringByStandardizingPath].stringByResolvingSymlinksInPath;
    int count = proc_listallpids(NULL, 0);
    if (count <= 0 || count >= INT_MAX / (2 * (int)sizeof(pid_t))) return nil;
    int capacity = count * 2;
    pid_t *pids = calloc((size_t)capacity, sizeof(pid_t));
    if (pids == NULL) return nil;
    int actual = proc_listallpids(pids, capacity * (int)sizeof(pid_t));
    if (actual <= 0 || actual >= capacity) {
        free(pids);
        return nil;
    }
    NSMutableArray<NSNumber *> *matches = [NSMutableArray array];
    for (int index = 0; index < actual; index++) {
        char executable[PROC_PIDPATHINFO_MAXSIZE] = {0};
        if (pids[index] <= 0 || proc_pidpath(pids[index], executable, sizeof(executable)) <= 0) continue;
        NSString *path = [NSString stringWithUTF8String:executable];
        path = path.stringByStandardizingPath.stringByResolvingSymlinksInPath;
        if ([path isEqualToString:expected]) [matches addObject:@(pids[index])];
    }
    free(pids);
    return matches.copy;
}

- (void)cancelRecoveryExitObservation {
    for (dispatch_source_t source in self.recoveryExitSources) dispatch_source_cancel(source);
    self.recoveryExitSources = nil;
}

- (void)resumeRecoveryAfterHostExit:(NSDictionary *)pending {
    if (self.recoveryExitSources == nil) return;
    [self cancelRecoveryExitObservation];
    if (![self pendingIsOwned:pending]) {
        [self stopCoordinator];
        return;
    }
    // launchRecovery重读注册；仍有同路径宿主时重新订阅，不调用helper。
    [self launchRecovery:pending relaunchHost:NO retry:0];
}

- (NSDictionary *)pendingWithoutHostRelaunch:(NSDictionary *)pending {
    __block NSDictionary *waiting = nil;
    [self withPendingLock:^BOOL{
        NSDictionary *current = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
        if (![self pending:current isOwnedBy:pending]) return NO;
        NSMutableDictionary *next = current.mutableCopy;
        // 宿主已在使用；将来正常关闭不再被本次交接自动重开。
        next[@"relaunchHost"] = @NO;
        if (![self writeJSON:next toPath:self.pendingPath]) return NO;
        waiting = next.copy;
        return YES;
    }];
    return waiting;
}

- (BOOL)deferRecoveryUntilHostExit:(NSDictionary *)pending {
    NSArray<NSNumber *> *pids = [self runningHostPIDs];
    if (pids.count == 0) return NO;
    NSDictionary *waiting = [self pendingWithoutHostRelaunch:pending];
    if (waiting == nil) {
        [self stopCoordinator];
        return YES;
    }
    [self cancelRecoveryExitObservation];
    NSMutableArray *sources = [NSMutableArray array];
    __weak typeof(self) weakSelf = self;
    for (NSNumber *pid in pids) {
        dispatch_source_t source = dispatch_source_create(DISPATCH_SOURCE_TYPE_PROC,
            (uintptr_t)pid.intValue, DISPATCH_PROC_EXIT, dispatch_get_main_queue());
        if (source == nil) {
            for (dispatch_source_t created in sources) {
                dispatch_source_cancel(created);
                dispatch_resume(created);
            }
            [self stopCoordinator];
            return YES;
        }
        dispatch_source_set_event_handler(source, ^{
            [weakSelf resumeRecoveryAfterHostExit:waiting];
        });
        [sources addObject:source];
    }
    self.recoveryExitSources = sources.copy;
    for (dispatch_source_t source in self.recoveryExitSources) dispatch_resume(source);
    incodexUpdateDiagnostic("recovery-wait-host-exit", pids.firstObject.intValue, (int)pids.count);
    // 订阅前的退出不能丢失；下一次helper仍执行真实quiescence与代际校验。
    NSArray<NSNumber *> *remaining = [self runningHostPIDs];
    if (remaining != nil && ![[NSSet setWithArray:remaining] isEqualToSet:[NSSet setWithArray:pids]]) {
        [self resumeRecoveryAfterHostExit:waiting];
    }
    return YES;
}

- (void)launchRecovery:(NSDictionary *)pending relaunchHost:(BOOL)relaunchHost retry:(NSUInteger)retry {
    if (![self pendingIsOwned:pending]) {
        [self stopCoordinator];
        return;
    }
    NSString *installID = pending[@"installId"];
    NSString *app = pending[@"appPath"];
    NSDictionary *registration = [self currentRegistrationForPending:pending];
    NSString *helper = registration[@"helperPath"];
    if (![installID isKindOfClass:NSString.class] || ![app isKindOfClass:NSString.class] ||
        ![helper isKindOfClass:NSString.class]) {
        incodexUpdateDiagnostic("registration-missing", self.hostPID, NO);
        [self finishRecovery:pending relaunchHost:relaunchHost success:NO];
        return;
    }
    incodexUpdateDiagnostic("registration-available", self.hostPID, YES);
    self.appPath = registration[@"appPath"];
    if ([self deferRecoveryUntilHostExit:pending]) return;
    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:helper];
    NSMutableDictionary *environment = NSProcessInfo.processInfo.environment.mutableCopy;
    environment[@"INCODEX_MACOS_UPDATE_RELAUNCH"] = @"1";
    environment[@"INCODEX_MACOS_UPDATE_INSTALL_ID"] = installID;
    environment[@"INCODEX_MACOS_UPDATE_HELPER_SHA256"] = registration[@"helperSha256"];
    task.environment = environment;
    task.standardOutput = [NSFileHandle fileHandleWithNullDevice];
    task.standardError = [NSFileHandle fileHandleWithNullDevice];
    __weak typeof(self) weakSelf = self;
    task.terminationHandler = ^(NSTask *finished) {
        dispatch_async(dispatch_get_main_queue(), ^{
            typeof(self) self = weakSelf;
            if (self == nil) return;
            incodexUpdateDiagnostic(
                "helper-exit",
                finished.processIdentifier,
                (int)finished.terminationStatus
            );
            if (finished.terminationStatus == 0) {
                [self finishRecovery:pending relaunchHost:relaunchHost success:YES];
                return;
            }
            // 明确事务前busy的结果不随宿主先退出而消失；只准一次空场重试。
            if (finished.terminationReason == NSTaskTerminationReasonExit &&
                finished.terminationStatus == EX_TEMPFAIL) {
                NSDictionary *waiting = [self pendingWithoutHostRelaunch:pending];
                if (waiting == nil) {
                    [self stopCoordinator];
                    return;
                }
                if ([self deferRecoveryUntilHostExit:waiting]) return;
                if (retry == 0) {
                    [self launchRecovery:waiting relaunchHost:NO retry:1];
                } else {
                    [self finishRecovery:waiting relaunchHost:NO success:NO];
                }
                return;
            }
            NSDictionary *latest = [self currentRegistrationForPending:pending];
            NSString *latestHelper = latest[@"helperPath"];
            if (retry == 0 && [latestHelper isKindOfClass:NSString.class] &&
                ![latestHelper isEqualToString:helper]) {
                [self launchRecovery:pending relaunchHost:relaunchHost retry:retry + 1];
                return;
            }
            [self finishRecovery:pending relaunchHost:relaunchHost success:NO];
        });
    };
    NSError *error = nil;
    if (![self launchTaskIfPendingOwned:pending task:task error:&error]) {
        NSDictionary *latest = [self currentRegistrationForPending:pending];
        NSString *latestHelper = latest[@"helperPath"];
        if (retry == 0 && [latestHelper isKindOfClass:NSString.class] &&
            ![latestHelper isEqualToString:helper]) {
            [self launchRecovery:pending relaunchHost:relaunchHost retry:retry + 1];
            return;
        }
        [self finishRecovery:pending relaunchHost:relaunchHost success:NO];
    }
}

- (void)recoverAfterUpdate:(NSDictionary *)pending relaunchHost:(BOOL)relaunchHost {
    [self launchRecovery:pending relaunchHost:relaunchHost retry:0];
}

- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    NSDictionary<NSString *, NSString *> *environment = NSProcessInfo.processInfo.environment;
    self.initialLaunch = [environment[@"INCODEX_MACOS_UPDATE_COORDINATOR"] isEqualToString:@"1"];
    if (self.initialLaunch) {
        self.hostPID = environment[@"INCODEX_MACOS_UPDATE_HOST_PID"].intValue;
        self.installID = environment[@"INCODEX_MACOS_UPDATE_INSTALL_ID"];
        self.appPath = environment[@"INCODEX_MACOS_UPDATE_HOST_APP"];
        self.helperPath = environment[@"INCODEX_MACOS_UPDATE_HELPER_PATH"];
        self.helperSha256 = environment[@"INCODEX_MACOS_UPDATE_HELPER_SHA256"];
        self.handoffId = environment[@"INCODEX_MACOS_UPDATE_HANDOFF_ID"];
        self.readyPath = environment[@"INCODEX_MACOS_UPDATE_READY_PATH"];
        self.pendingPath = environment[@"INCODEX_MACOS_UPDATE_PENDING_PATH"];
        if (self.hostPID <= 0 || self.installID.length == 0 || self.appPath.length == 0 ||
            self.helperPath.length == 0 || self.helperSha256.length == 0 || self.handoffId.length == 0 ||
            self.readyPath.length == 0 || self.pendingPath.length == 0) {
            [self stopCoordinator];
            return;
        }
        self.sourceBuild = [self currentHostBuild];
        if (self.sourceBuild.length == 0) {
            [self stopCoordinator];
            return;
        }
        if (![self persistPending]) {
            [self stopCoordinator];
            return;
        }
        [self armHostExitObservation];
        [self writeReady];
        return;
    }

    self.pendingPath = incodexUpdatePendingPathForCoordinator(NSBundle.mainBundle.bundlePath);
    NSDictionary *pending = [self loadAndAdoptPending];
    if (pending == nil) {
        incodexUpdateDiagnostic("recovery-pending-absent", self.hostPID, NO);
        [self stopCoordinator];
        return;
    }
    incodexUpdateDiagnostic("recovery-pending-adopted", self.hostPID, YES);
    BOOL relaunchHost = ![pending[@"relaunchHost"] isEqual:@NO];
    [self recoverAfterUpdate:pending relaunchHost:relaunchHost];
}

@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc == 3 && strcmp(argv[1], "--continue-update") == 0) {
            char *end = NULL;
            long parentPID = strtol(argv[2], &end, 10);
            if (end == argv[2] || *end != '\0' || parentPID <= 0 || parentPID > INT_MAX) return 1;
            IncodexUpdateDelegate *delegate = [IncodexUpdateDelegate new];
            [delegate runContinuationAfterParent:(pid_t)parentPID];
            // 不创建NSApplication，避免被Sparkle的退出目标枚举再次选中。
            dispatch_main();
        }
        NSApplication *application = NSApplication.sharedApplication;
        application.activationPolicy = NSApplicationActivationPolicyProhibited;
        IncodexUpdateDelegate *delegate = [[IncodexUpdateDelegate alloc] init];
        application.delegate = delegate;
        [application run];
    }
    return 0;
}
