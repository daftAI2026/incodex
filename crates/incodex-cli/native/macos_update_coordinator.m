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

@interface IncodexUpdateDelegate : NSObject <NSApplicationDelegate>
@property(nonatomic) BOOL initialLaunch;
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
            [NSApp replyToApplicationShouldTerminate:YES];
            return;
        }
        if ([self hostBundleChanged]) {
            [self recoverAfterUpdate:[self pendingSnapshot] relaunchHost:NO];
            return;
        }
        [self removePendingIfOwned:[self pendingSnapshot]];
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

- (NSDictionary *)readPrivateJSONAtPath:(NSString *)path maxBytes:(off_t)maxBytes {
    int descriptor = open(path.fileSystemRepresentation, O_RDONLY | O_NOFOLLOW);
    if (descriptor < 0) return nil;

    struct stat metadata;
    if (fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_uid != geteuid() || (metadata.st_mode & 0077) != 0 ||
        metadata.st_size <= 0 || metadata.st_size > maxBytes) {
        close(descriptor);
        return nil;
    }

    NSFileHandle *handle = [[NSFileHandle alloc] initWithFileDescriptor:descriptor closeOnDealloc:YES];
    NSData *data = [handle readDataToEndOfFile];
    id body = data == nil ? nil : [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    return [body isKindOfClass:NSDictionary.class] ? body : nil;
}

- (BOOL)withPendingLock:(BOOL (^)(void))body {
    NSString *directory = self.pendingPath.stringByDeletingLastPathComponent;
    NSString *lockPath = [directory stringByAppendingPathComponent:@".pending.lock"];
    int descriptor = open(lockPath.fileSystemRepresentation, O_CREAT | O_RDWR | O_NOFOLLOW, 0600);
    if (descriptor < 0) return NO;
    struct stat metadata;
    BOOL valid = fstat(descriptor, &metadata) == 0 && S_ISREG(metadata.st_mode) &&
                 metadata.st_uid == geteuid() && (metadata.st_mode & 0777) == 0600;
    if (!valid || flock(descriptor, LOCK_EX) != 0) {
        close(descriptor);
        return NO;
    }
    BOOL result = body();
    flock(descriptor, LOCK_UN);
    close(descriptor);
    return result;
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
    __block NSDictionary *adopted = nil;
    [self withPendingLock:^BOOL{
        NSDictionary *current = [self readPrivateJSONAtPath:self.pendingPath maxBytes:64 * 1024];
        if (current == nil || ![current[@"installId"] isKindOfClass:NSString.class] ||
            ![current[@"appPath"] isKindOfClass:NSString.class]) return NO;
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
            [NSApp terminate:nil];
            return;
        }
    } else if (![self pendingIsOwned:pending]) {
        [NSApp terminate:nil];
        return;
    }
    if (relaunchHost && success && self.appPath.length > 0) {
        [self launchHostAndExit];
    } else {
        self.allowTermination = YES;
        [NSApp terminate:nil];
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
    return launched;
}

- (void)launchRecovery:(NSDictionary *)pending relaunchHost:(BOOL)relaunchHost retry:(NSUInteger)retry {
    if (![self pendingIsOwned:pending]) {
        [NSApp terminate:nil];
        return;
    }
    NSString *installID = pending[@"installId"];
    NSString *app = pending[@"appPath"];
    NSDictionary *registration = [self currentRegistrationForPending:pending];
    NSString *helper = registration[@"helperPath"];
    if (![installID isKindOfClass:NSString.class] || ![app isKindOfClass:NSString.class] ||
        ![helper isKindOfClass:NSString.class]) {
        [self finishRecovery:pending relaunchHost:relaunchHost success:NO];
        return;
    }
    self.appPath = registration[@"appPath"];
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
            if (finished.terminationStatus == 0) {
                [self finishRecovery:pending relaunchHost:relaunchHost success:YES];
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
    NSDictionary *pending = [self loadAndAdoptPending];
    if (pending == nil) {
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
