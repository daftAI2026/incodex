/**
 * [INPUT]: 依赖 AppKit 进程生命周期、受控 pending 状态与固定 Incodex Helper
 * [OUTPUT]: 提供 Sparkle application bundle 替身，覆盖交互式重启与后台更新后的静默恢复
 * [POS]: macOS 更新链的原生 Coordinator；只编排退出和恢复，不实现 ASAR 修改或签名策略
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <AppKit/AppKit.h>
#import <Foundation/Foundation.h>
#import <dispatch/dispatch.h>
#import <signal.h>
#import <sys/stat.h>

@interface IncodexUpdateDelegate : NSObject <NSApplicationDelegate>
@property(nonatomic) BOOL initialLaunch;
@property(nonatomic) pid_t hostPID;
@property(nonatomic, copy) NSString *installID;
@property(nonatomic, copy) NSString *appPath;
@property(nonatomic, copy) NSString *helperPath;
@property(nonatomic, copy) NSString *readyPath;
@property(nonatomic, copy) NSString *pendingPath;
@property(nonatomic, copy) NSString *sourceBuild;
@property(nonatomic) BOOL terminationPending;
@property(nonatomic) BOOL allowTermination;
@property(nonatomic) dispatch_source_t hostExitSource;
@end

@implementation IncodexUpdateDelegate

- (BOOL)processIsRunning:(pid_t)pid {
    return pid > 0 && (kill(pid, 0) == 0 || errno == EPERM);
}

- (BOOL)writeJSON:(NSDictionary *)body toPath:(NSString *)path {
    NSError *error = nil;
    NSData *data = [NSJSONSerialization dataWithJSONObject:body options:0 error:&error];
    if (data == nil || error != nil) return NO;
    NSString *directory = path.stringByDeletingLastPathComponent;
    if (![NSFileManager.defaultManager createDirectoryAtPath:directory
                                  withIntermediateDirectories:YES
                                                   attributes:@{NSFilePosixPermissions: @0700}
                                                        error:&error]) return NO;
    if (![data writeToFile:path options:NSDataWritingAtomic error:&error]) return NO;
    return chmod(path.fileSystemRepresentation, 0600) == 0;
}

- (void)writeReady {
    NSDictionary *ready = @{ @"schemaVersion": @1, @"pid": @(NSProcessInfo.processInfo.processIdentifier) };
    if (![self writeJSON:ready toPath:self.readyPath]) {
        [NSApp terminate:nil];
    }
}

- (BOOL)persistPending {
    NSDictionary *pending = @{
        @"schemaVersion": @1,
        @"installId": self.installID,
        @"appPath": self.appPath,
        @"helperPath": self.helperPath,
    };
    return [self writeJSON:pending toPath:self.pendingPath];
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
            [NSApp replyToApplicationShouldTerminate:YES];
            return;
        }
        if ([self hostBundleChanged]) {
            NSDictionary *pending = @{
                @"installId": self.installID,
                @"appPath": self.appPath,
                @"helperPath": self.helperPath,
            };
            [self recoverAfterUpdate:pending relaunchHost:NO];
            return;
        }
        [NSFileManager.defaultManager removeItemAtPath:self.pendingPath error:nil];
        self.allowTermination = YES;
        [NSApp terminate:nil];
    });
    dispatch_resume(self.hostExitSource);
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
        [NSApp terminate:nil];
    }];
}

- (void)recoverAfterUpdate:(NSDictionary *)pending relaunchHost:(BOOL)relaunchHost {
    NSString *helper = pending[@"helperPath"];
    NSString *installID = pending[@"installId"];
    NSString *app = pending[@"appPath"];
    if (![helper isKindOfClass:NSString.class] || ![installID isKindOfClass:NSString.class] ||
        ![app isKindOfClass:NSString.class]) {
        [NSFileManager.defaultManager removeItemAtPath:self.pendingPath error:nil];
        [NSApp terminate:nil];
        return;
    }
    self.appPath = app;
    NSTask *task = [[NSTask alloc] init];
    task.executableURL = [NSURL fileURLWithPath:helper];
    NSMutableDictionary *environment = NSProcessInfo.processInfo.environment.mutableCopy;
    environment[@"INCODEX_MACOS_UPDATE_RELAUNCH"] = @"1";
    environment[@"INCODEX_MACOS_UPDATE_INSTALL_ID"] = installID;
    task.environment = environment;
    task.standardOutput = [NSFileHandle fileHandleWithNullDevice];
    task.standardError = [NSFileHandle fileHandleWithNullDevice];
    __weak typeof(self) weakSelf = self;
    task.terminationHandler = ^(__unused NSTask *finished) {
        dispatch_async(dispatch_get_main_queue(), ^{
            typeof(self) self = weakSelf;
            if (self == nil) return;
            [NSFileManager.defaultManager removeItemAtPath:self.pendingPath error:nil];
            if (relaunchHost) {
                [self launchHostAndExit];
            } else {
                self.allowTermination = YES;
                [NSApp terminate:nil];
            }
        });
    };
    NSError *error = nil;
    if (![task launchAndReturnError:&error]) {
        [NSFileManager.defaultManager removeItemAtPath:self.pendingPath error:nil];
        if (relaunchHost) {
            [self launchHostAndExit];
        } else {
            self.allowTermination = YES;
            [NSApp terminate:nil];
        }
    }
}

- (void)applicationDidFinishLaunching:(NSNotification *)notification {
    NSDictionary<NSString *, NSString *> *environment = NSProcessInfo.processInfo.environment;
    self.initialLaunch = [environment[@"INCODEX_MACOS_UPDATE_COORDINATOR"] isEqualToString:@"1"];
    if (self.initialLaunch) {
        self.hostPID = environment[@"INCODEX_MACOS_UPDATE_HOST_PID"].intValue;
        self.installID = environment[@"INCODEX_MACOS_UPDATE_INSTALL_ID"];
        self.appPath = environment[@"INCODEX_MACOS_UPDATE_HOST_APP"];
        self.helperPath = environment[@"INCODEX_MACOS_UPDATE_HELPER_PATH"];
        self.readyPath = environment[@"INCODEX_MACOS_UPDATE_READY_PATH"];
        self.pendingPath = environment[@"INCODEX_MACOS_UPDATE_PENDING_PATH"];
        if (self.hostPID <= 0 || self.installID.length == 0 || self.appPath.length == 0 ||
            self.helperPath.length == 0 || self.readyPath.length == 0 || self.pendingPath.length == 0) {
            [NSApp terminate:nil];
            return;
        }
        self.sourceBuild = [self currentHostBuild];
        if (self.sourceBuild.length == 0) {
            [NSApp terminate:nil];
            return;
        }
        if (![self persistPending]) {
            [NSApp terminate:nil];
            return;
        }
        [self armHostExitObservation];
        [self writeReady];
        return;
    }

    NSString *root = NSBundle.mainBundle.bundlePath;
    for (NSUInteger index = 0; index < 4; index += 1) {
        root = root.stringByDeletingLastPathComponent;
    }
    self.pendingPath = [root stringByAppendingPathComponent:@"macos-update/pending.json"];
    NSData *data = [NSData dataWithContentsOfFile:self.pendingPath];
    NSDictionary *pending = data == nil ? nil : [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    if (![pending isKindOfClass:NSDictionary.class]) {
        [NSApp terminate:nil];
        return;
    }
    [self recoverAfterUpdate:pending relaunchHost:YES];
}

@end

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        NSApplication *application = NSApplication.sharedApplication;
        application.activationPolicy = NSApplicationActivationPolicyProhibited;
        IncodexUpdateDelegate *delegate = [[IncodexUpdateDelegate alloc] init];
        application.delegate = delegate;
        [application run];
    }
    return 0;
}
