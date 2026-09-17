#import <Cocoa/Cocoa.h>
#import <objc/message.h>
#import <objc/runtime.h>
#include <dlfcn.h>
#include <ctype.h>
#include <math.h>
#include <stdio.h>
#include <string.h>

static int fail(const char *message) {
    fprintf(stderr, "permission native ABI smoke: %s\n", message);
    return 1;
}

static void normalize_encoding(const char *input, char *output, size_t capacity) {
    size_t index = 0;
    while (*input != '\0' && index + 1 < capacity) {
        if (!isdigit((unsigned char)*input)) output[index++] = *input;
        input += 1;
    }
    output[index] = '\0';
}

static BOOL require_encoding(Class cls, SEL selector, const char *expected) {
    Method method = class_getInstanceMethod(cls, selector);
    const char *actual = method == NULL ? NULL : method_getTypeEncoding(method);
    char normalized[256];
    normalize_encoding(actual == NULL ? "" : actual, normalized, sizeof(normalized));
    BOOL matches = actual != NULL && strcmp(normalized, expected) == 0;
    // Swift Bool is encoded as B on arm64 and c on x86_64.
#if defined(__x86_64__)
    if (!matches && strcmp(expected, "v@:ddB") == 0) {
        matches = strcmp(normalized, "v@:ddc") == 0;
    }
#endif
    if (!matches) {
        fprintf(
            stderr,
            "permission native ABI smoke: selector %s encoding %s (expected %s)\n",
            sel_getName(selector),
            actual == NULL ? "<missing>" : actual,
            expected
        );
        return NO;
    }
    return YES;
}

static BOOL require_class(const char *name, Class *result) {
    Class cls = NSClassFromString([NSString stringWithUTF8String:name]);
    if (cls == Nil) {
        fprintf(stderr, "permission native ABI smoke: class %s is missing\n", name);
        return NO;
    }
    *result = cls;
    return YES;
}

static BOOL hosting_view(NSView *view) {
    // A host may specialize AppKit geometry without replacing its SwiftUI
    // renderer. Verify its actual superclass chain, not just the leaf name.
    for (Class cls = view.class; cls != Nil; cls = class_getSuperclass(cls)) {
        if ([NSStringFromClass(cls) containsString:@"NSHostingView"]) return YES;
    }
    return NO;
}

int main(int argc, const char *argv[]) {
    if (argc != 2) return fail("expected the committed dylib path");
    @autoreleasepool {
        if (![NSThread isMainThread]) return fail("must run on the main thread");
        (void)[NSApplication sharedApplication];

        void *handle = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
        if (handle == NULL) {
            fprintf(stderr, "permission native ABI smoke: dlopen failed: %s\n", dlerror());
            return 2;
        }

        Class flightClass = Nil;
        Class initialClass = Nil;
        Class helperClass = Nil;
        if (!require_class("IncodexPermissionFlightView", &flightClass)
            || !require_class("IncodexPermissionInitialView", &initialClass)
            || !require_class("IncodexPermissionHelperView", &helperClass)) {
            return 3;
        }

        SEL flightProgress = @selector(flightProgress);
        SEL flightCornerRadius = @selector(flightCornerRadius);
        SEL setSource = @selector(setSourceImage:targetImage:);
        SEL updateProgress = @selector(updateProgress:cornerRadius:reduceTransparency:);
        if (!require_encoding(flightClass, flightProgress, "d@:")
            || !require_encoding(flightClass, flightCornerRadius, "d@:")
            || !require_encoding(flightClass, setSource, "v@:@@")
            || !require_encoding(flightClass, updateProgress, "v@:ddB")) {
            return 4;
        }

        id flight = ((id (*)(id, SEL, NSRect))objc_msgSend)(
            [flightClass alloc],
            @selector(initWithFrame:),
            NSMakeRect(0, 0, 128, 128)
        );
        if (flight == nil) return fail("flight init failed");
        ((void (*)(id, SEL, id, id))objc_msgSend)(flight, setSource, nil, nil);
        ((void (*)(id, SEL, double, double, BOOL))objc_msgSend)(
            flight, updateProgress, 0.25, 24.0, YES
        );
        double observedProgress = ((double (*)(id, SEL))objc_msgSend)(flight, flightProgress);
        double observedRadius = ((double (*)(id, SEL))objc_msgSend)(flight, flightCornerRadius);
        if (fabs(observedProgress - 0.25) > 1e-12 || fabs(observedRadius - 24.0) > 1e-12) {
            return fail("flight progress or corner-radius bridge mismatch");
        }

        NSDictionary *copy = @{
            @"title": @"Enable ChatGPT scripting",
            @"body": @"Allow Accessibility access.",
            @"permissionTitle": @"Accessibility",
            @"permissionDescription": @"Read and control app interfaces",
            @"repair": @"Allow",
            @"later": @"Skip",
            @"completeInSettings": @"Complete in Settings",
            @"back": @"Back",
            @"dragInstruction": @"Drag ChatGPT into the app list above.",
        };

        SEL initialConfigure = @selector(configureWithCopy:appIcon:permissionIcon:actionTarget:);
        SEL initialCard = @selector(permissionCardView);
        SEL initialSize = @selector(preferredContentSize);
        if (!require_encoding(initialClass, initialConfigure, "v@:@@@@")
            || !require_encoding(initialClass, initialCard, "@@:")
            || !require_encoding(initialClass, initialSize, "{CGSize=dd}@:")) {
            return 5;
        }
        NSView *initial = ((id (*)(id, SEL, NSRect))objc_msgSend)(
            [initialClass alloc],
            @selector(initWithFrame:),
            NSMakeRect(0, 0, 600, 340)
        );
        if (initial == nil) return fail("initial init failed");
        ((void (*)(id, SEL, id, id, id, id))objc_msgSend)(
            initial, initialConfigure, copy, nil, nil, nil
        );
        NSView *initialCardView = ((id (*)(id, SEL))objc_msgSend)(initial, initialCard);
        NSSize initialSizeValue = ((NSSize (*)(id, SEL))objc_msgSend)(initial, initialSize);
        if (!hosting_view(initialCardView)
            || initialSizeValue.width != 600
            || initialSizeValue.height < 312
            || initial.subviews.count == 0
            || !hosting_view(initial.subviews[0])) {
            return fail("initial card/host/size ABI contract failed");
        }

        SEL helperConfigure = @selector(configureWithCopy:appIcon:actionTarget:);
        SEL helperRow = @selector(appRowView);
        SEL helperFrame = @selector(appRowFrame);
        SEL helperSize = @selector(preferredContentSize);
        SEL helperSnapshot = @selector(snapshotImageWithScale:);
        if (!require_encoding(helperClass, helperConfigure, "v@:@@@")
            || !require_encoding(helperClass, helperRow, "@@:")
            || !require_encoding(helperClass, helperFrame, "{CGRect={CGPoint=dd}{CGSize=dd}}@:")
            || !require_encoding(helperClass, helperSize, "{CGSize=dd}@:")
            || !require_encoding(helperClass, helperSnapshot, "@@:d")) {
            return 6;
        }
        NSView *helper = ((id (*)(id, SEL, NSRect))objc_msgSend)(
            [helperClass alloc],
            @selector(initWithFrame:),
            NSMakeRect(0, 0, 531, 110)
        );
        if (helper == nil) return fail("helper init failed");
        ((void (*)(id, SEL, id, id, id))objc_msgSend)(helper, helperConfigure, copy, nil, nil);
        NSView *rowView = ((id (*)(id, SEL))objc_msgSend)(helper, helperRow);
#if defined(__x86_64__)
        NSRect rowFrame = ((NSRect (*)(id, SEL))objc_msgSend_stret)(helper, helperFrame);
#else
        NSRect rowFrame = ((NSRect (*)(id, SEL))objc_msgSend)(helper, helperFrame);
#endif
        NSSize helperSizeValue = ((NSSize (*)(id, SEL))objc_msgSend)(helper, helperSize);
        if (!hosting_view(rowView)
            || rowFrame.size.width != 459
            || rowFrame.size.height != 42
            || helperSizeValue.width != 531
            || helperSizeValue.height < 110
            || helper.subviews.count == 0
            || !hosting_view(helper.subviews[0])) {
            return fail("helper row/host/size ABI contract failed");
        }

        NSImage *snapshot = ((id (*)(id, SEL, double))objc_msgSend)(helper, helperSnapshot, 2.0);
        if (snapshot == nil || !NSEqualSizes(snapshot.size, helperSizeValue)) {
            return fail("helper foreground snapshot ABI contract failed");
        }
        for (NSWindow *window in NSApp.windows) {
            if (window.visible) return fail("ABI smoke unexpectedly showed a window");
        }
        puts("permission native Objective-C ABI smoke passed");
        return 0;
    }
}
