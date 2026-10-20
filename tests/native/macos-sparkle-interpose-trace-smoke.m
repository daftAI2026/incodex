/**
 * [INPUT]: 依赖interposer、共用pending锁、Foundation与私有假App。
 * [OUTPUT]: 校验回调保真、排队身份与自有异常后的锁释放。
 * [POS]: tests/native 的隔离原生回归；只写私有临时夹具，不启动官方 App 或权限流程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import "macos_update_pending.h"
#import <dlfcn.h>
#import <fcntl.h>
#import <objc/runtime.h>
#import <sys/file.h>
#import <sys/stat.h>
#import <unistd.h>

typedef void (^ImmediateInstallationBlock)(void);

@class SUAppcastItem;
@class CodexSparkleDelegate;

static NSUInteger gInitializerCalls = 0;
static __strong id gInitializerReceiver = nil;
static SEL gInitializerSelector = NULL;
static __strong NSBundle *gInitializerHostBundle = nil;
static __strong NSBundle *gInitializerApplicationBundle = nil;
static __strong id gInitializerUserDriver = nil;
static __strong id gInitializerDelegate = nil;

static NSUInteger gDelegateCalls = 0;
static __strong id gDelegateReceiver = nil;
static SEL gDelegateSelector = NULL;
static __strong id gDelegateUpdater = nil;
static __strong SUAppcastItem *gDelegateUpdateItem = nil;
static void *gDelegateBlockAddress = NULL;
static BOOL gDelegateResult = NO;
static BOOL gDelegateInvokesBlock = NO;
static BOOL gDelegateThrows = NO;
static NSUInteger gBlockCalls = 0;
static __strong NSException *gOriginalException = nil;
static __strong CodexSparkleDelegate *gTestDelegate = nil;
static NSString *gPendingPath = nil;
static NSString *gPendingLockPath = nil;
static NSDictionary *gPendingSeed = nil;
static NSDictionary *gPendingUnqueued = nil;

@interface SPUUpdater : NSObject
- (instancetype)initWithHostBundle:(NSBundle *)hostBundle
                  applicationBundle:(NSBundle *)applicationBundle
                         userDriver:(id)userDriver
                           delegate:(id)delegate;
@end

@implementation SPUUpdater
- (instancetype)initWithHostBundle:(NSBundle *)hostBundle
                  applicationBundle:(NSBundle *)applicationBundle
                         userDriver:(id)userDriver
                           delegate:(id)delegate {
    gInitializerCalls += 1;
    gInitializerReceiver = self;
    gInitializerSelector = _cmd;
    gInitializerHostBundle = hostBundle;
    gInitializerApplicationBundle = applicationBundle;
    gInitializerUserDriver = userDriver;
    gInitializerDelegate = delegate;
    return self;
}
@end

@interface SUAppcastItem : NSObject
@end

@implementation SUAppcastItem
@end

@interface CodexSparkleDelegate : NSObject
- (BOOL)updater:(id)updater
    willInstallUpdateOnQuit:(SUAppcastItem *)updateItem
    immediateInstallationBlock:(ImmediateInstallationBlock)block;
@end

@implementation CodexSparkleDelegate
- (BOOL)updater:(id)updater
    willInstallUpdateOnQuit:(SUAppcastItem *)updateItem
    immediateInstallationBlock:(ImmediateInstallationBlock)block {
    gDelegateCalls += 1;
    gDelegateReceiver = self;
    gDelegateSelector = _cmd;
    gDelegateUpdater = updater;
    gDelegateUpdateItem = updateItem;
    gDelegateBlockAddress = (__bridge void *)block;
    if (gDelegateThrows) {
        @throw gOriginalException;
    }
    if (gDelegateInvokesBlock && block != nil) {
        block();
    }
    return gDelegateResult;
}
@end

static int fail(NSString *message) {
    fprintf(stderr, "FIXTURE_ASSERTION_FAILED: %s\n", message.UTF8String);
    return 2;
}

static NSBundle *bundleFromEnvironment(NSString *name) {
    NSString *path = NSProcessInfo.processInfo.environment[name];
    NSBundle *bundle = path.length == 0 ? nil : [NSBundle bundleWithPath:path];
    if (bundle == nil || bundle.executablePath.length == 0) {
        return nil;
    }
    return bundle;
}

static SPUUpdater *newUpdater(
    NSBundle *hostBundle,
    NSBundle *applicationBundle,
    id userDriver,
    id delegate
) {
    return [[SPUUpdater alloc] initWithHostBundle:hostBundle
                                 applicationBundle:applicationBundle
                                        userDriver:userDriver
                                          delegate:delegate];
}

static BOOL samePath(NSString *left, NSString *right) {
    return [left.stringByStandardizingPath.stringByResolvingSymlinksInPath
        isEqualToString:right.stringByStandardizingPath.stringByResolvingSymlinksInPath];
}

static NSString *pendingPathForCoordinator(NSBundle *coordinatorBundle) {
    NSString *root = coordinatorBundle.bundlePath;
    for (NSUInteger index = 0; index < 4; index += 1) {
        root = root.stringByDeletingLastPathComponent;
    }
    return [root stringByAppendingPathComponent:@"macos-update/pending.json"];
}

static BOOL withFixturePendingLock(BOOL (^body)(void)) {
    int descriptor = open(gPendingLockPath.fileSystemRepresentation,
                          O_CREAT | O_RDWR | O_NOFOLLOW | O_CLOEXEC,
                          0600);
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

static NSDictionary *readFixturePendingUnlocked(void) {
    int descriptor = open(gPendingPath.fileSystemRepresentation, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    if (descriptor < 0) return nil;
    struct stat metadata;
    if (fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_uid != geteuid() || (metadata.st_mode & 0777) != 0600 ||
        metadata.st_size <= 0 || metadata.st_size > 16 * 1024) {
        close(descriptor);
        return nil;
    }
    NSFileHandle *handle = [[NSFileHandle alloc] initWithFileDescriptor:descriptor
                                                         closeOnDealloc:YES];
    NSData *data = [handle readDataToEndOfFile];
    id value = data == nil ? nil :
        [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    return [value isKindOfClass:NSDictionary.class] ? value : nil;
}

static BOOL writeFixturePendingUnlocked(NSDictionary *pending) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:pending
                                                   options:NSJSONWritingSortedKeys
                                                     error:nil];
    if (data == nil || ![data writeToFile:gPendingPath
                                  options:NSDataWritingAtomic
                                    error:nil]) {
        return NO;
    }
    return chmod(gPendingPath.fileSystemRepresentation, 0600) == 0;
}

static NSDictionary *fixturePendingSnapshot(void) {
    __block NSDictionary *snapshot = nil;
    BOOL locked = withFixturePendingLock(^BOOL{
        snapshot = readFixturePendingUnlocked();
        return snapshot != nil;
    });
    return locked ? snapshot : nil;
}

static BOOL fixturePendingMatchesSeed(NSDictionary *pending) {
    for (NSString *key in @[@"schemaVersion", @"installId", @"appPath", @"handoffId",
                             @"coordinatorPid", @"sourceBuild"]) {
        if (![pending[key] isEqual:gPendingSeed[key]]) return NO;
    }
    return [pending[@"installQueued"] isEqual:@YES];
}

static BOOL preparePendingFixture(void) {
    NSDictionary<NSString *, NSString *> *environment = NSProcessInfo.processInfo.environment;
    NSString *home = environment[@"HOME"];
    NSString *hostPath = environment[@"INCODEX_TEST_HOST_APP"];
    NSString *coordinatorPath = environment[@"INCODEX_TEST_COORDINATOR_APP"];
    NSBundle *hostBundle = hostPath.length == 0 ? nil : [NSBundle bundleWithPath:hostPath];
    NSBundle *coordinatorBundle = coordinatorPath.length == 0
        ? nil : [NSBundle bundleWithPath:coordinatorPath];
    if (home.length == 0 || hostBundle == nil || coordinatorBundle == nil) {
        fail(@"pending fixture requires the isolated HOME and both fake bundles");
        return NO;
    }

    gPendingPath = pendingPathForCoordinator(coordinatorBundle);
    gPendingLockPath = [gPendingPath.stringByDeletingLastPathComponent
        stringByAppendingPathComponent:@".pending.lock"];
    NSString *expectedPath = [[home stringByAppendingPathComponent:@".incodex"]
        stringByAppendingPathComponent:@"macos-update/pending.json"];
    if (!samePath(gPendingPath, expectedPath) ||
        ![hostBundle.bundleIdentifier isEqualToString:@"com.openai.codex"]) {
        fail(@"pending fixture must resolve under its isolated HOME and fake Codex bundle");
        return NO;
    }

    NSDictionary *hostInfo = [NSDictionary dictionaryWithContentsOfFile:
        [hostBundle.bundlePath stringByAppendingPathComponent:@"Contents/Info.plist"]];
    id sourceBuild = hostInfo[@"CFBundleVersion"];
    if (![sourceBuild isKindOfClass:NSString.class] || [(NSString *)sourceBuild length] == 0) {
        fail(@"fake Host must provide its current source build");
        return NO;
    }

    NSDictionary *seed = @{
        @"schemaVersion": @2,
        @"installId": [NSString stringWithFormat:@"fixture-install-%d", getpid()],
        @"appPath": hostBundle.bundlePath,
        @"handoffId": [NSString stringWithFormat:@"fixture-handoff-%d", getpid()],
        @"coordinatorPid": @(getpid()),
        @"sourceBuild": sourceBuild,
    };
    gPendingSeed = seed;
    if (!withFixturePendingLock(^BOOL{
            return writeFixturePendingUnlocked(seed);
        })) {
        fail(@"schema-2 pending and its 0600 lock must be created in the fixture HOME");
        return NO;
    }
    NSDictionary *snapshot = fixturePendingSnapshot();
    if (![snapshot isEqualToDictionary:gPendingSeed]) {
        fail(@"pending seed must contain only the owned schema-2 handoff identity");
        return NO;
    }
    return YES;
}

static BOOL resetQueuedFieldForNonmatch(void) {
    __block BOOL reset = NO;
    BOOL locked = withFixturePendingLock(^BOOL{
        NSDictionary *current = readFixturePendingUnlocked();
        if (current == nil) return NO;
        NSMutableDictionary *next = current.mutableCopy;
        [next removeObjectForKey:@"installQueued"];
        if (!writeFixturePendingUnlocked(next)) return NO;
        gPendingUnqueued = readFixturePendingUnlocked();
        reset = [gPendingUnqueued isEqualToDictionary:next];
        return reset;
    });
    return locked && reset;
}

static int exerciseMatchedInitializerAndDelegate(NSString *mode) {
    NSBundle *hostBundle = bundleFromEnvironment(@"INCODEX_TEST_HOST_APP");
    NSBundle *coordinatorBundle = bundleFromEnvironment(@"INCODEX_TEST_COORDINATOR_APP");
    if (hostBundle == nil || coordinatorBundle == nil) {
        return fail(@"matched fixture bundles must load an executable path");
    }

    id userDriver = [NSObject new];
    CodexSparkleDelegate *delegate = [mode isEqualToString:@"matched-no-delegate"]
        ? nil : [CodexSparkleDelegate new];
    gTestDelegate = delegate;
    SPUUpdater *updater = newUpdater(hostBundle, hostBundle, userDriver, delegate);
    if (updater == nil || gInitializerCalls != 1 || gInitializerReceiver != updater) {
        return fail(@"the original initializer must run once on the original receiver");
    }
    if (gInitializerSelector != @selector(initWithHostBundle:applicationBundle:userDriver:delegate:)) {
        return fail(@"the original initializer selector must remain unchanged");
    }
    if (gInitializerHostBundle != hostBundle || gInitializerUserDriver != userDriver ||
        gInitializerDelegate != delegate) {
        return fail(@"the original initializer must receive the exact original arguments");
    }
    if (gInitializerApplicationBundle == hostBundle ||
        !samePath(gInitializerApplicationBundle.bundlePath, coordinatorBundle.bundlePath)) {
        return fail(@"the exact matched host and application must receive the coordinator bundle");
    }
    if (delegate == nil) {
        return 0;
    }

    BOOL expectedResult = [mode isEqualToString:@"matched-yes"] ||
        [mode isEqualToString:@"pending-yes"];
    gDelegateResult = expectedResult;
    gDelegateInvokesBlock = expectedResult;
    gDelegateThrows = [mode isEqualToString:@"matched-exception"] ||
        [mode isEqualToString:@"pending-exception"];
    gOriginalException = [NSException exceptionWithName:@"FixtureOriginalException"
                                                 reason:@"fixture exception identity"
                                               userInfo:nil];
    SUAppcastItem *updateItem = [SUAppcastItem new];
    ImmediateInstallationBlock block = ^{ gBlockCalls += 1; };

    if (gDelegateThrows) {
        @try {
            [delegate updater:updater
                willInstallUpdateOnQuit:updateItem
                immediateInstallationBlock:block];
            return fail(@"the original delegate exception must propagate");
        } @catch (NSException *caught) {
            if (caught != gOriginalException) {
                return fail(@"the exact original exception object must propagate");
            }
        }
    } else {
        BOOL result = [delegate updater:updater
              willInstallUpdateOnQuit:updateItem
              immediateInstallationBlock:block];
        if (result != expectedResult) {
            return fail(@"the original delegate BOOL result must remain unchanged");
        }
    }

    if (gDelegateCalls != 1 || gDelegateReceiver != delegate ||
        gDelegateSelector != @selector(updater:willInstallUpdateOnQuit:immediateInstallationBlock:) ||
        gDelegateUpdater != updater || gDelegateUpdateItem != updateItem ||
        gDelegateBlockAddress != (__bridge void *)block) {
        return fail(@"the original delegate self, selector and arguments must remain unchanged");
    }
    NSUInteger expectedBlockCalls = expectedResult && !gDelegateThrows ? 1 : 0;
    if (gBlockCalls != expectedBlockCalls) {
        return fail(@"the immediate block call count must remain owned by the original delegate");
    }
    if (gPendingPath != nil) {
        NSDictionary *pending = fixturePendingSnapshot();
        BOOL pendingIsValid = gDelegateThrows
            ? [pending isEqualToDictionary:gPendingSeed]
            : fixturePendingMatchesSeed(pending);
        if (!pendingIsValid) {
            return fail(gDelegateThrows
                ? @"an exception must not queue the pending handoff"
                : @"a matched callback must queue without changing its pending identity");
        }
    }
    return 0;
}

static void resetFixtureCounters(void) {
    gInitializerCalls = 0;
    gInitializerReceiver = nil;
    gInitializerSelector = NULL;
    gInitializerHostBundle = nil;
    gInitializerApplicationBundle = nil;
    gInitializerUserDriver = nil;
    gInitializerDelegate = nil;
    gDelegateCalls = 0;
    gDelegateReceiver = nil;
    gDelegateSelector = NULL;
    gDelegateUpdater = nil;
    gDelegateUpdateItem = nil;
    gDelegateBlockAddress = NULL;
    gDelegateResult = NO;
    gDelegateInvokesBlock = NO;
    gDelegateThrows = NO;
    gBlockCalls = 0;
    gOriginalException = nil;
}

static int exerciseNonmatchingUpdaters(BOOL reuseDelegate) {
    NSBundle *hostBundle = bundleFromEnvironment(@"INCODEX_TEST_HOST_APP");
    NSBundle *wrongHostBundle = bundleFromEnvironment(@"INCODEX_TEST_WRONG_HOST_APP");
    NSBundle *wrongApplicationBundle = bundleFromEnvironment(@"INCODEX_TEST_WRONG_APPLICATION_APP");
    NSBundle *coordinatorBundle = bundleFromEnvironment(@"INCODEX_TEST_COORDINATOR_APP");
    if (hostBundle == nil || wrongHostBundle == nil || wrongApplicationBundle == nil ||
        coordinatorBundle == nil) {
        return fail(@"nonmatching fixture bundles must load executable paths");
    }

    NSUInteger initializerBaseline = gInitializerCalls;
    NSUInteger delegateBaseline = gDelegateCalls;
    NSUInteger blockBaseline = gBlockCalls;
    id userDriver = [NSObject new];
    CodexSparkleDelegate *delegate = reuseDelegate ? gTestDelegate : [CodexSparkleDelegate new];
    gTestDelegate = delegate;
    if (delegate == nil) {
        return fail(@"the matched delegate instance must remain available for nonmatch checks");
    }
    SPUUpdater *wrongHostUpdater = newUpdater(wrongHostBundle, wrongHostBundle, userDriver, delegate);
    if (gInitializerApplicationBundle != wrongHostBundle ||
        samePath(gInitializerApplicationBundle.bundlePath, coordinatorBundle.bundlePath)) {
        return fail(@"a nonmatching host must keep the original application bundle");
    }

    SPUUpdater *wrongApplicationUpdater = newUpdater(hostBundle, wrongApplicationBundle, userDriver, delegate);
    if (gInitializerApplicationBundle != wrongApplicationBundle ||
        samePath(gInitializerApplicationBundle.bundlePath, coordinatorBundle.bundlePath)) {
        return fail(@"a nonmatching application bundle must remain unchanged");
    }
    if (gInitializerCalls != initializerBaseline + 2 || gInitializerReceiver != wrongApplicationUpdater) {
        return fail(@"both nonmatching updater initializers must run once on their receivers");
    }

    SUAppcastItem *updateItem = [SUAppcastItem new];
    ImmediateInstallationBlock block = ^{ gBlockCalls += 1; };
    gDelegateResult = YES;
    gDelegateInvokesBlock = YES;
    BOOL firstResult = [delegate updater:wrongHostUpdater
                 willInstallUpdateOnQuit:updateItem
                 immediateInstallationBlock:block];
    BOOL secondResult = [delegate updater:wrongApplicationUpdater
                  willInstallUpdateOnQuit:updateItem
                  immediateInstallationBlock:block];
    if (!firstResult || !secondResult || gDelegateCalls != delegateBaseline + 2 ||
        gBlockCalls != blockBaseline + 2 || gDelegateReceiver != delegate ||
        gDelegateUpdater != wrongApplicationUpdater || gDelegateUpdateItem != updateItem ||
        gDelegateBlockAddress != (__bridge void *)block) {
        return fail(@"nonmatching callbacks must preserve the delegate's original behavior");
    }
    return 0;
}

static BOOL pendingLockIsAvailable(void) {
    int descriptor = open(gPendingLockPath.fileSystemRepresentation, O_RDWR | O_NOFOLLOW);
    if (descriptor < 0) return NO;
    BOOL available = flock(descriptor, LOCK_EX | LOCK_NB) == 0;
    if (available) flock(descriptor, LOCK_UN);
    close(descriptor);
    return available;
}

static int exercisePendingLockException(void) {
    if (!incodexUpdateWithPendingLock(gPendingPath, ^BOOL{ return YES; }) ||
        !pendingLockIsAvailable()) {
        return fail(@"normal lock body must release the shared OS lock as a positive control");
    }
    NSException *expected = [NSException exceptionWithName:@"OwnedPendingException"
                                                   reason:nil userInfo:nil];
    BOOL caught = NO;
    @try {
        incodexUpdateWithPendingLock(gPendingPath, ^BOOL{
            @throw expected;
            return NO;
        });
    } @catch (id actual) {
        if (actual != expected) return fail(@"shared lock must preserve the exact body exception");
        caught = YES;
    }
    if (!caught || !pendingLockIsAvailable()) {
        return fail(@"an owned body exception must not retain the shared pending OS lock");
    }
    return 0;
}

static int exercisePendingNonmatchingCallbacks(void) {
    if (!resetQueuedFieldForNonmatch()) {
        return fail(@"the matched-case queue field must reset before the nonmatching callback");
    }
    resetFixtureCounters();
    int status = exerciseNonmatchingUpdaters(YES);
    if (status != 0) return status;
    NSDictionary *pending = fixturePendingSnapshot();
    if (![pending isEqualToDictionary:gPendingUnqueued]) {
        return fail(@"a nonmatching updater must leave the reset pending record unchanged");
    }
    return 0;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 2) {
            return fail(@"expected one fixture mode");
        }
        NSString *mode = [NSString stringWithUTF8String:argv[1]];
        BOOL pendingMode = [mode hasPrefix:@"pending-"];
        if (pendingMode && !preparePendingFixture()) {
            return 2;
        }
        const char *interposerPath = getenv("INCODEX_TEST_INTERPOSER_DYLIB");
        if (interposerPath == NULL || dlopen(interposerPath, RTLD_NOW) == NULL) {
            return fail(@"the compiled native interposer must load into this fixture process");
        }
        int status = 0;
        if ([mode isEqualToString:@"pending-lock-exception"]) {
            status = exercisePendingLockException();
        } else if (pendingMode) {
            status = exerciseMatchedInitializerAndDelegate(mode);
            if (status == 0 && ![mode isEqualToString:@"pending-exception"]) {
                status = exercisePendingNonmatchingCallbacks();
            }
        } else if ([mode isEqualToString:@"nonmatch-many"]) {
            status = exerciseNonmatchingUpdaters(NO);
        } else if ([mode isEqualToString:@"matched-then-nonmatch"]) {
            status = exerciseMatchedInitializerAndDelegate(@"matched-yes");
            if (status == 0) {
                resetFixtureCounters();
                status = exerciseNonmatchingUpdaters(YES);
            }
        } else {
            status = exerciseMatchedInitializerAndDelegate(mode);
        }
        if (status != 0) {
            return status;
        }
        printf("FIXTURE_PASS: %s\n", mode.UTF8String);
        return 0;
    }
}
