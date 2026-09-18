#import <Cocoa/Cocoa.h>
#import <ApplicationServices/ApplicationServices.h>
#import <objc/message.h>
#include <dlfcn.h>
#include <math.h>
#include <stdio.h>
#include <string.h>
#include <unistd.h>

@interface BackTarget : NSObject
@property(nonatomic) NSInteger laterCount;
@end

@implementation BackTarget
- (void)later:(id)sender {
    (void)sender;
    self.laterCount += 1;
}
@end

// This is a deliberately small real-window probe.  Its AX traversal follows
// /tmp/incodex-ax-probe.m, but it retains only the text/frame operations needed
// for P11 so a failing layout assertion is easy to audit.

static CFTypeRef axAttribute(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = NULL;
    if (element != NULL && AXUIElementCopyAttributeValue(element, name, &value) == kAXErrorSuccess) {
        return value;
    }
    return NULL;
}

static NSString *axString(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = axAttribute(element, name);
    if (value == NULL) return @"";
    NSString *result = CFGetTypeID(value) == CFStringGetTypeID()
        ? [(__bridge NSString *)value copy]
        : [[(__bridge id)value description] copy];
    CFRelease(value);
    return result;
}

static BOOL axRect(AXUIElementRef element, NSRect *result) {
    CFTypeRef positionValue = axAttribute(element, kAXPositionAttribute);
    CFTypeRef sizeValue = axAttribute(element, kAXSizeAttribute);
    BOOL valid = NO;
    if (positionValue != NULL && sizeValue != NULL
        && CFGetTypeID(positionValue) == AXValueGetTypeID()
        && CFGetTypeID(sizeValue) == AXValueGetTypeID()) {
        CGPoint position = CGPointZero;
        CGSize size = CGSizeZero;
        valid = AXValueGetValue((AXValueRef)positionValue, kAXValueCGPointType, &position)
            && AXValueGetValue((AXValueRef)sizeValue, kAXValueCGSizeType, &size);
        if (valid) {
            result->origin = NSMakePoint(position.x, position.y);
            result->size = NSMakeSize(size.width, size.height);
        }
    }
    if (positionValue != NULL) CFRelease(positionValue);
    if (sizeValue != NULL) CFRelease(sizeValue);
    return valid;
}

static BOOL axHasText(AXUIElementRef element, NSString *text) {
    if (text == nil) return YES;
    for (NSString *attribute in @[
        (__bridge NSString *)kAXDescriptionAttribute,
        (__bridge NSString *)kAXTitleAttribute,
        (__bridge NSString *)kAXValueAttribute,
    ]) {
        if ([axString(element, (__bridge CFStringRef)attribute) isEqualToString:text]) return YES;
    }
    return NO;
}

static AXUIElementRef findAX(AXUIElementRef root, CFStringRef role, NSString *text,
                             CGFloat width, CGFloat height, BOOL matchSize, int depth) {
    if (root == NULL || depth > 30) return NULL;

    NSString *actualRole = axString(root, kAXRoleAttribute);
    BOOL roleMatches = role == NULL || [actualRole isEqualToString:(__bridge NSString *)role];
    NSRect frame = NSZeroRect;
    BOOL sizeMatches = !matchSize || (axRect(root, &frame)
                                      && fabs(frame.size.width - width) < 1.0
                                      && fabs(frame.size.height - height) < 1.0);
    if (roleMatches && sizeMatches && axHasText(root, text)) {
        return (AXUIElementRef)CFRetain(root);
    }

    CFTypeRef childrenValue = axAttribute(root, kAXChildrenAttribute);
    if (childrenValue != NULL && CFGetTypeID(childrenValue) == CFArrayGetTypeID()) {
        CFArrayRef children = (CFArrayRef)childrenValue;
        for (CFIndex index = 0; index < CFArrayGetCount(children); index += 1) {
            AXUIElementRef child = (AXUIElementRef)CFArrayGetValueAtIndex(children, index);
            AXUIElementRef found = findAX(child, role, text, width, height, matchSize, depth + 1);
            if (found != NULL) {
                CFRelease(childrenValue);
                return found;
            }
        }
    }
    if (childrenValue != NULL) CFRelease(childrenValue);
    return NULL;
}

static BOOL axContainsText(AXUIElementRef root, NSString *text, int depth) {
    if (root == NULL || depth > 30) return NO;
    if (axHasText(root, text)) return YES;
    CFTypeRef childrenValue = axAttribute(root, kAXChildrenAttribute);
    if (childrenValue != NULL && CFGetTypeID(childrenValue) == CFArrayGetTypeID()) {
        CFArrayRef children = (CFArrayRef)childrenValue;
        for (CFIndex index = 0; index < CFArrayGetCount(children); index += 1) {
            if (axContainsText(
                    (AXUIElementRef)CFArrayGetValueAtIndex(children, index), text, depth + 1)) {
                CFRelease(childrenValue);
                return YES;
            }
        }
    }
    if (childrenValue != NULL) CFRelease(childrenValue);
    return NO;
}

static AXUIElementRef findWindow(AXUIElementRef application, NSString *text) {
    CFTypeRef windowsValue = axAttribute(application, kAXWindowsAttribute);
    if (windowsValue == NULL || CFGetTypeID(windowsValue) != CFArrayGetTypeID()) {
        if (windowsValue != NULL) CFRelease(windowsValue);
        return NULL;
    }
    CFArrayRef windows = (CFArrayRef)windowsValue;
    AXUIElementRef result = NULL;
    for (CFIndex index = 0; index < CFArrayGetCount(windows); index += 1) {
        AXUIElementRef window = (AXUIElementRef)CFArrayGetValueAtIndex(windows, index);
        if ([axString(window, kAXRoleAttribute) isEqualToString:(__bridge NSString *)kAXWindowRole]
            && axContainsText(window, text, 0)) {
            result = (AXUIElementRef)CFRetain(window);
            break;
        }
    }
    CFRelease(windowsValue);
    return result;
}

static BOOL rectanglesOverlap(NSRect first, NSRect second) {
    NSRect intersection = NSIntersectionRect(first, second);
    return intersection.size.width > 0.5 && intersection.size.height > 0.5;
}

static BOOL approximately(CGFloat actual, CGFloat expected) {
    return fabs(actual - expected) <= 0.5;
}

static void drainRunLoop(NSTimeInterval seconds) {
    NSDate *deadline = [NSDate dateWithTimeIntervalSinceNow:seconds];
    while ([deadline timeIntervalSinceNow] > 0) {
        [[NSRunLoop currentRunLoop] runMode:NSDefaultRunLoopMode beforeDate:deadline];
    }
}

static NSSize preferredContentSize(id view) {
    // NSSize is two CGFloat values (16 bytes on macOS); it uses the ordinary
    // objc_msgSend ABI on both supported slices.  Only larger structs such as
    // NSRect need a stret call on Intel.
    return ((NSSize (*)(id, SEL))objc_msgSend)(view, @selector(preferredContentSize));
}

static id makeView(Class cls, NSRect frame) {
    return ((id (*)(id, SEL, NSRect))objc_msgSend)([cls alloc], @selector(initWithFrame:), frame);
}

static NSDictionary *longTitleCopy(void) {
    return @{
        @"title": @"Enable ChatGPT scripting after a deliberately long localized permission title that should wrap",
        @"body": @"Allow Accessibility access.",
        @"permissionTitle": @"Accessibility",
        @"permissionDescription": @"Read and control app interfaces",
        @"repair": @"Allow",
        @"later": @"Skip",
        @"completeInSettings": @"Complete in Settings",
        @"back": @"Back",
        @"dragInstruction": @"Drag ChatGPT into the app list above.",
    };
}

static NSDictionary *shortTitleCopy(void) {
    return @{
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
}

int main(int argc, const char **argv) {
    if (argc < 2 || argc > 3) {
        fprintf(stderr, "usage: %s /path/to/incodex-permission-ui.dylib [card|back]\n", argv[0]);
        return 2;
    }
    BOOL cardMode = argc == 3 && strcmp(argv[2], "card") == 0;
    BOOL backMode = argc == 3 && strcmp(argv[2], "back") == 0;
    if (argc == 3 && !cardMode && !backMode) {
        fprintf(stderr, "unknown layout smoke mode: %s\n", argv[2]);
        return 2;
    }

    @autoreleasepool {
        if (![NSThread isMainThread]) return 2;
        [NSApplication sharedApplication];
        [NSApp finishLaunching];
        [NSApp setActivationPolicy:NSApplicationActivationPolicyAccessory];
        [NSApp activateIgnoringOtherApps:YES];

        void *handle = dlopen(argv[1], RTLD_NOW | RTLD_LOCAL);
        if (handle == NULL) {
            fprintf(stderr, "P11 dlopen failed: %s\n", dlerror());
            return 3;
        }

        Class initialClass = NSClassFromString(@"IncodexPermissionInitialView");
        Class helperClass = NSClassFromString(@"IncodexPermissionHelperView");
        if ((!backMode && initialClass == Nil) || (backMode && helperClass == Nil)) {
            dlclose(handle);
            return 4;
        }

        if (backMode) {
            NSDictionary *copy = shortTitleCopy();
            BackTarget *target = [BackTarget new];
            id helper = makeView(helperClass, NSMakeRect(0, 0, 531, 110));
            ((void (*)(id, SEL, id, id, id))objc_msgSend)(
                helper,
                @selector(configureWithCopy:appIcon:actionTarget:),
                copy,
                nil,
                target
            );
            NSSize size = preferredContentSize(helper);
            [helper setFrameSize:size];

            NSPanel *panel = [[NSPanel alloc] initWithContentRect:NSMakeRect(240, 520, size.width, size.height)
                                                         styleMask:NSWindowStyleMaskNonactivatingPanel
                                                           backing:NSBackingStoreBuffered
                                                             defer:NO];
            panel.releasedWhenClosed = NO;
            panel.contentView = helper;
            panel.level = NSNormalWindowLevel;
            [panel makeKeyAndOrderFront:nil];
            drainRunLoop(0.35);

            AXUIElementRef application = AXUIElementCreateApplication(getpid());
            AXUIElementRef window = findWindow(application, copy[@"dragInstruction"]);
            AXUIElementRef back = findAX(window, kAXButtonRole, copy[@"back"], 0, 0, NO, 0);
            NSRect windowRect = NSZeroRect;
            NSRect backRect = NSZeroRect;
            BOOL geometryFound = window != NULL && back != NULL
                && axRect(window, &windowRect)
                && axRect(back, &backRect);
            CGFloat backX = backRect.origin.x - windowRect.origin.x;
            CGFloat backY = backRect.origin.y - windowRect.origin.y;
            BOOL backGeometryOK = geometryFound
                && approximately(backX, 18.0)
                && approximately(backY, 55.0)
                && approximately(backRect.size.width, 28.0)
                && approximately(backRect.size.height, 28.0);
            printf(
                "BACK_AX window=(%.1f,%.1f %.1fx%.1f) backRel=(%.1f,%.1f %.1fx%.1f)\n",
                windowRect.origin.x, windowRect.origin.y, windowRect.size.width, windowRect.size.height,
                backX, backY, backRect.size.width, backRect.size.height
            );
            BOOL pressed = back != NULL
                && AXUIElementPerformAction(back, kAXPressAction) == kAXErrorSuccess;
            drainRunLoop(0.1);
            BOOL callbackOK = pressed && target.laterCount == 1;
            printf(
                "BACK_CHECK geometry=%s pressed=%s laterCount=%ld\n",
                backGeometryOK ? "yes" : "no",
                pressed ? "yes" : "no",
                (long)target.laterCount
            );
            if (!geometryFound) {
                fprintf(stderr, "Back failed: real AX helper window/button node was not found\n");
            }
            if (!backGeometryOK) {
                fprintf(stderr, "Back failed: AX button frame is not relative 18,55,28x28\n");
            }
            if (!callbackOK) {
                fprintf(stderr, "Back failed: AXPress did not dispatch later exactly once\n");
            }

            if (back != NULL) CFRelease(back);
            if (window != NULL) CFRelease(window);
            if (application != NULL) CFRelease(application);
            [panel orderOut:nil];
            [panel close];
            return backGeometryOK && callbackOK ? 0 : 1;
        }

        NSDictionary *copy = cardMode ? shortTitleCopy() : longTitleCopy();
        id initial = makeView(initialClass, NSMakeRect(0, 0, 600, 340));
        ((void (*)(id, SEL, id, id, id, id))objc_msgSend)(
            initial,
            @selector(configureWithCopy:appIcon:permissionIcon:actionTarget:),
            copy,
            nil,
            nil,
            nil
        );
        NSSize size = preferredContentSize(initial);
        [initial setFrameSize:size];

        NSPanel *panel = [[NSPanel alloc] initWithContentRect:NSMakeRect(240, 520, size.width, size.height)
                                                     styleMask:NSWindowStyleMaskTitled
                                                       backing:NSBackingStoreBuffered
                                                         defer:NO];
        panel.releasedWhenClosed = NO;
        panel.contentView = initial;
        panel.level = NSNormalWindowLevel;
        [panel makeKeyAndOrderFront:nil];
        drainRunLoop(0.35);

        AXUIElementRef application = AXUIElementCreateApplication(getpid());
        AXUIElementRef window = findWindow(application, copy[@"title"]);
        AXUIElementRef card = findAX(window, kAXGroupRole, nil, 518, 80, YES, 0);

        if (cardMode) {
            AXUIElementRef cardTitle = findAX(card, kAXStaticTextRole, copy[@"permissionTitle"], 0, 0, NO, 0);
            AXUIElementRef cardDescription = findAX(card, kAXStaticTextRole, copy[@"permissionDescription"], 0, 0, NO, 0);
            AXUIElementRef allow = findAX(card, kAXButtonRole, copy[@"repair"], 0, 0, NO, 0);
            NSRect cardRect = NSZeroRect;
            NSRect titleRect = NSZeroRect;
            NSRect descriptionRect = NSZeroRect;
            NSRect allowRect = NSZeroRect;
            BOOL geometryFound = card != NULL
                && cardTitle != NULL
                && cardDescription != NULL
                && allow != NULL
                && axRect(card, &cardRect)
                && axRect(cardTitle, &titleRect)
                && axRect(cardDescription, &descriptionRect)
                && axRect(allow, &allowRect);
            CGFloat titleX = titleRect.origin.x - cardRect.origin.x;
            CGFloat titleY = titleRect.origin.y - cardRect.origin.y;
            CGFloat descriptionX = descriptionRect.origin.x - cardRect.origin.x;
            CGFloat descriptionY = descriptionRect.origin.y - cardRect.origin.y;
            CGFloat allowX = allowRect.origin.x - cardRect.origin.x;
            CGFloat allowY = allowRect.origin.y - cardRect.origin.y;
            BOOL cardSizeOK = geometryFound
                && approximately(cardRect.size.width, 518.0)
                && approximately(cardRect.size.height, 80.0);
            BOOL titleOK = geometryFound
                && approximately(titleX, 84.5)
                && approximately(titleY, 20.5);
            BOOL descriptionOK = geometryFound
                && approximately(descriptionX, 84.5)
                && approximately(descriptionY, 42.5);
            BOOL allowOK = geometryFound
                && approximately(allowX, 443.0)
                && approximately(allowY, 28.0)
                && approximately(allowRect.size.width, 56.5)
                && approximately(allowRect.size.height, 24.0);
            printf(
                "C03_C11_AX card=(%.1f,%.1f %.1fx%.1f) titleRel=(%.1f,%.1f) descRel=(%.1f,%.1f) allowRel=(%.1f,%.1f %.1fx%.1f)\n",
                cardRect.origin.x, cardRect.origin.y, cardRect.size.width, cardRect.size.height,
                titleX, titleY, descriptionX, descriptionY,
                allowX, allowY, allowRect.size.width, allowRect.size.height
            );
            printf(
                "C03_C11_CHECK card=%s title=%s description=%s allow=%s\n",
                cardSizeOK ? "yes" : "no",
                titleOK ? "yes" : "no",
                descriptionOK ? "yes" : "no",
                allowOK ? "yes" : "no"
            );
            if (!geometryFound) {
                fprintf(stderr, "C03/C11 failed: real AX card/title/description/Allow node was not found\n");
            }
            if (!cardSizeOK) fprintf(stderr, "C03/C11 failed: card AX frame is not 518x80\n");
            if (!titleOK) fprintf(stderr, "C03/C11 failed: card title relative origin is not 84.5,20.5\n");
            if (!descriptionOK) fprintf(stderr, "C03/C11 failed: card description relative origin is not 84.5,42.5\n");
            if (!allowOK) fprintf(stderr, "C03/C11 failed: Allow relative frame is not 443,28,56.5,24\n");

            if (cardTitle != NULL) CFRelease(cardTitle);
            if (cardDescription != NULL) CFRelease(cardDescription);
            if (allow != NULL) CFRelease(allow);
            if (card != NULL) CFRelease(card);
            if (window != NULL) CFRelease(window);
            if (application != NULL) CFRelease(application);
            [panel orderOut:nil];
            [panel close];
            return cardSizeOK && titleOK && descriptionOK && allowOK ? 0 : 1;
        }

        AXUIElementRef title = findAX(window, kAXStaticTextRole, copy[@"title"], 0, 0, NO, 0);
        AXUIElementRef body = findAX(window, kAXStaticTextRole, copy[@"body"], 0, 0, NO, 0);
        NSRect titleRect = NSZeroRect;
        NSRect bodyRect = NSZeroRect;
        NSRect cardRect = NSZeroRect;
        BOOL foundGeometry = title != NULL && body != NULL && card != NULL
            && axRect(title, &titleRect)
            && axRect(body, &bodyRect)
            && axRect(card, &cardRect);

        BOOL titleWidthOK = foundGeometry && titleRect.size.width <= 560.0;
        BOOL titleHeightOK = foundGeometry && titleRect.size.height > 30.0;
        BOOL noOverlap = foundGeometry
            && !rectanglesOverlap(titleRect, bodyRect)
            && !rectanglesOverlap(titleRect, cardRect);
        printf(
            "P11_AX title=(%.1f,%.1f %.1fx%.1f) body=(%.1f,%.1f %.1fx%.1f) card=(%.1f,%.1f %.1fx%.1f)\n",
            titleRect.origin.x, titleRect.origin.y, titleRect.size.width, titleRect.size.height,
            bodyRect.origin.x, bodyRect.origin.y, bodyRect.size.width, bodyRect.size.height,
            cardRect.origin.x, cardRect.origin.y, cardRect.size.width, cardRect.size.height
        );
        printf(
            "P11_CHECK titleWidthLE560=%s titleHeightGT30=%s titleNoOverlap=%s\n",
            titleWidthOK ? "yes" : "no",
            titleHeightOK ? "yes" : "no",
            noOverlap ? "yes" : "no"
        );

        if (title == NULL || body == NULL || card == NULL) {
            fprintf(stderr, "P11 failed: real AX title/body/card node was not found\n");
        }
        if (!titleWidthOK) {
            fprintf(stderr, "P11 failed: title AX width %.1f exceeds 560\n", titleRect.size.width);
        }
        if (!titleHeightOK) {
            fprintf(stderr, "P11 failed: title AX height %.1f does not prove wrapping\n", titleRect.size.height);
        }
        if (!noOverlap) {
            fprintf(stderr, "P11 failed: title overlaps body or permission card\n");
        }

        if (title != NULL) CFRelease(title);
        if (body != NULL) CFRelease(body);
        if (card != NULL) CFRelease(card);
        if (window != NULL) CFRelease(window);
        if (application != NULL) CFRelease(application);
        [panel orderOut:nil];
        [panel close];
        // Keep the SwiftUI/Objective-C objects and their code image loaded;
        // process exit reclaims this probe.  Unloading here can race object
        // teardown on the main run loop.

        return titleWidthOK && titleHeightOK && noOverlap ? 0 : 1;
    }
}
