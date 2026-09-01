#import <Foundation/Foundation.h>
#import <objc/runtime.h>

typedef id (*UpdaterInitializer)(id, SEL, NSBundle *, NSBundle *, id, id);

static UpdaterInitializer originalInitializer = NULL;
static NSString *expectedHostPath = nil;
static NSString *coordinatorAppPath = nil;

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
    NSString *hostIdentifier = hostBundle.bundleIdentifier;
    NSString *hostPath = canonicalPath(hostBundle.bundlePath);
    NSString *applicationPath = canonicalPath(applicationBundle.bundlePath);
    if ([hostIdentifier isEqualToString:@"com.openai.codex"] &&
        [hostPath isEqualToString:expectedHostPath] &&
        [applicationPath isEqualToString:expectedHostPath]) {
        NSBundle *candidate = [NSBundle bundleWithPath:coordinatorAppPath];
        if (candidate.executablePath.length > 0) {
            replacement = candidate;
        }
    }
    return originalInitializer(receiver, selector, hostBundle, replacement, userDriver, delegate);
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
}
