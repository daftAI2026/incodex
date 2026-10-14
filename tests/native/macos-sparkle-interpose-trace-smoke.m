/**
 * [INPUT]: 依赖 native/macos_sparkle_interpose.m、Foundation 与临时 App bundle。
 * [OUTPUT]: 校验 Sparkle 初始化器替换及委托回调保持原参数、结果与异常。
 * [POS]: tests/native 的隔离原生回归；不启动 Sparkle 更新、官方 App 或权限流程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <dlfcn.h>
#import <objc/runtime.h>

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

    BOOL expectedResult = [mode isEqualToString:@"matched-yes"];
    gDelegateResult = expectedResult;
    gDelegateInvokesBlock = expectedResult;
    gDelegateThrows = [mode isEqualToString:@"matched-exception"];
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

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 2) {
            return fail(@"expected one fixture mode");
        }
        const char *interposerPath = getenv("INCODEX_TEST_INTERPOSER_DYLIB");
        if (interposerPath == NULL || dlopen(interposerPath, RTLD_NOW) == NULL) {
            return fail(@"the compiled native interposer must load into this fixture process");
        }
        NSString *mode = [NSString stringWithUTF8String:argv[1]];
        int status = 0;
        if ([mode isEqualToString:@"nonmatch-many"]) {
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
