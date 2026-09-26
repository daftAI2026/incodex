#import <Cocoa/Cocoa.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CoreGraphics/CoreGraphics.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static CFTypeRef attribute(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = NULL;
    if (element && AXUIElementCopyAttributeValue(element, name, &value) == kAXErrorSuccess) return value;
    return NULL;
}

static NSString *stringAttribute(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = attribute(element, name);
    if (!value) return @"";
    NSString *result = CFGetTypeID(value) == CFStringGetTypeID()
        ? (__bridge NSString *)value
        : [(__bridge id)value description];
    NSString *copy = [result copy];
    CFRelease(value);
    return copy;
}

static NSString *normalized(NSString *value) {
    NSArray *parts = [value componentsSeparatedByCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    NSMutableArray *nonEmpty = [NSMutableArray array];
    for (NSString *part in parts) if (part.length) [nonEmpty addObject:part];
    return [nonEmpty componentsJoinedByString:@" "];
}

static BOOL matches(AXUIElementRef element, NSString *expected) {
    if (!expected.length) return YES;
    CFStringRef keys[] = { kAXDescriptionAttribute, kAXTitleAttribute, kAXValueAttribute };
    for (NSUInteger index = 0; index < sizeof(keys) / sizeof(keys[0]); index++) {
        NSString *actual = stringAttribute(element, keys[index]);
        if ([actual isEqualToString:expected] || [normalized(actual) isEqualToString:normalized(expected)]) return YES;
    }
    return NO;
}

static AXUIElementRef findElement(AXUIElementRef root, CFStringRef role, NSString *expected, int depth) {
    if (!root || depth > 32) return NULL;
    BOOL roleMatches = !role || [stringAttribute(root, kAXRoleAttribute) isEqualToString:(__bridge NSString *)role];
    if (roleMatches && matches(root, expected)) return (AXUIElementRef)CFRetain(root);
    CFTypeRef childrenValue = attribute(root, kAXChildrenAttribute);
    if (childrenValue && CFGetTypeID(childrenValue) == CFArrayGetTypeID()) {
        CFArrayRef children = (CFArrayRef)childrenValue;
        for (CFIndex index = 0; index < CFArrayGetCount(children); index++) {
            AXUIElementRef child = (AXUIElementRef)CFArrayGetValueAtIndex(children, index);
            AXUIElementRef found = findElement(child, role, expected, depth + 1);
            if (found) { CFRelease(childrenValue); return found; }
        }
    }
    if (childrenValue) CFRelease(childrenValue);
    return NULL;
}

static BOOL elementRect(AXUIElementRef element, CGRect *result) {
    CFTypeRef position = attribute(element, kAXPositionAttribute);
    CFTypeRef size = attribute(element, kAXSizeAttribute);
    BOOL valid = NO;
    if (position && size && CFGetTypeID(position) == AXValueGetTypeID() && CFGetTypeID(size) == AXValueGetTypeID()) {
        CGPoint point = CGPointZero;
        CGSize extent = CGSizeZero;
        valid = AXValueGetValue((AXValueRef)position, kAXValueCGPointType, &point) &&
            AXValueGetValue((AXValueRef)size, kAXValueCGSizeType, &extent);
        if (valid) *result = CGRectMake(point.x, point.y, extent.width, extent.height);
    }
    if (position) CFRelease(position);
    if (size) CFRelease(size);
    return valid;
}

static NSDictionary *rectObject(CGRect rect) {
    return @{@"x": @(rect.origin.x), @"y": @(rect.origin.y),
             @"width": @(rect.size.width), @"height": @(rect.size.height)};
}

static NSDictionary *windowInfo(pid_t pid, NSString *title) {
    CFArrayRef list = CGWindowListCopyWindowInfo(kCGWindowListOptionAll, kCGNullWindowID);
    if (!list) return nil;
    NSDictionary *fallback = nil;
    for (NSDictionary *entry in (__bridge NSArray *)list) {
        NSNumber *owner = entry[(id)kCGWindowOwnerPID];
        NSNumber *onscreen = entry[(id)kCGWindowIsOnscreen];
        if (owner.intValue != pid) continue;
        NSString *name = entry[(id)kCGWindowName] ?: @"";
        if ([name isEqualToString:title]) { fallback = entry; break; }
        if (!fallback && onscreen.boolValue) fallback = entry;
    }
    NSDictionary *copy = [fallback copy];
    CFRelease(list);
    return copy;
}

static CGImageRef captureWindow(CGWindowID windowID, NSString **errorMessage) {
    // Preflight is read-only. Do not call any ScreenCaptureKit API unless this
    // process already has screen-recording permission; the diagnostic never
    // requests or changes TCC permissions.
    if (!CGPreflightScreenCaptureAccess()) {
        if (errorMessage) *errorMessage = @"screen capture permission is not already granted";
        return NULL;
    }
    if (@available(macOS 14.0, *)) {
        dispatch_semaphore_t completed = dispatch_semaphore_create(0);
        __block CGImageRef captured = NULL;
        __block NSString *failure = nil;
        [SCShareableContent getShareableContentExcludingDesktopWindows:YES onScreenWindowsOnly:YES
            completionHandler:^(SCShareableContent *content, NSError *error) {
                if (!content) {
                    failure = error.localizedDescription ?: @"ScreenCaptureKit could not enumerate windows";
                    dispatch_semaphore_signal(completed);
                    return;
                }
                SCWindow *target = nil;
                for (SCWindow *candidate in content.windows) {
                    if (candidate.windowID == windowID) { target = candidate; break; }
                }
                if (!target) {
                    failure = @"ScreenCaptureKit did not expose the isolated host window";
                    dispatch_semaphore_signal(completed);
                    return;
                }
                SCContentFilter *filter = [[SCContentFilter alloc] initWithDesktopIndependentWindow:target];
                SCStreamConfiguration *configuration = [SCStreamConfiguration new];
                configuration.width = MAX(1, (size_t)ceil(target.frame.size.width * 2));
                configuration.height = MAX(1, (size_t)ceil(target.frame.size.height * 2));
                configuration.showsCursor = NO;
                [SCScreenshotManager captureImageWithFilter:filter configuration:configuration
                    completionHandler:^(CGImageRef image, NSError *captureError) {
                        if (image) captured = CGImageRetain(image);
                        else failure = captureError.localizedDescription ?: @"ScreenCaptureKit returned no image";
                        dispatch_semaphore_signal(completed);
                    }];
            }];
        long wait = dispatch_semaphore_wait(completed, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
        if (wait != 0 && !failure) failure = @"ScreenCaptureKit capture timed out";
        if (errorMessage) *errorMessage = failure;
        return captured;
    }
    if (errorMessage) *errorMessage = @"ScreenCaptureKit image capture requires macOS 14 or later";
    return NULL;
}

static NSDictionary *pixelSample(CGWindowID windowID, NSDictionary *window, CGRect buttonRect, NSString *snapshotPath) {
    NSString *captureError = nil;
    CGImageRef image = captureWindow(windowID, &captureError);
    if (!image) return @{@"pixelKnown": @NO, @"captureError": captureError ?: @"ScreenCaptureKit returned nil"};

    NSMutableDictionary *result = [NSMutableDictionary dictionary];
    result[@"pixelKnown"] = @NO;
    size_t imageWidth = CGImageGetWidth(image);
    size_t imageHeight = CGImageGetHeight(image);
    result[@"imageWidth"] = @(imageWidth);
    result[@"imageHeight"] = @(imageHeight);

    NSBitmapImageRep *bitmap = [[NSBitmapImageRep alloc] initWithCGImage:image];
    NSData *png = [bitmap representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    if (snapshotPath.length && png && [png writeToFile:snapshotPath atomically:YES]) {
        result[@"snapshot"] = snapshotPath;
    } else if (snapshotPath.length) {
        result[@"snapshotError"] = @"PNG snapshot could not be written";
    }

    NSDictionary *bounds = window[(id)kCGWindowBounds];
    CGFloat originX = [bounds[@"X"] doubleValue];
    CGFloat originY = [bounds[@"Y"] doubleValue];
    CGFloat width = [bounds[@"Width"] doubleValue];
    CGFloat height = [bounds[@"Height"] doubleValue];
    if (width > 0 && height > 0 && imageWidth && imageHeight) {
        CGFloat sampleX = buttonRect.origin.x + MIN(5.0, buttonRect.size.width * 0.1);
        CGFloat sampleY = CGRectGetMidY(buttonRect);
        NSInteger x = (NSInteger)floor((sampleX - originX) * imageWidth / width);
        NSInteger y = (NSInteger)floor((sampleY - originY) * imageHeight / height);
        x = MAX(0, MIN((NSInteger)imageWidth - 1, x));
        y = MAX(0, MIN((NSInteger)imageHeight - 1, y));
        NSColor *color = [[bitmap colorAtX:x y:y] colorUsingColorSpace:[NSColorSpace deviceRGBColorSpace]];
        if (color) {
            result[@"pixelKnown"] = @YES;
            result[@"samplePoint"] = @{@"x": @(sampleX), @"y": @(sampleY), @"imageX": @(x), @"imageY": @(y)};
            result[@"rgb"] = @{@"r": @(color.redComponent), @"g": @(color.greenComponent), @"b": @(color.blueComponent)};
        }
    }
    if (![result[@"pixelKnown"] boolValue]) result[@"captureError"] = @"Could not sample the Allow background pixel";
    CGImageRelease(image);
    return result;
}

static BOOL postMouse(CGEventType type, CGPoint point) {
    CGEventRef event = CGEventCreateMouseEvent(NULL, type, point, kCGMouseButtonLeft);
    if (!event) return NO;
    CGEventPost(kCGHIDEventTap, event);
    CFRelease(event);
    return YES;
}

static void printObject(NSDictionary *object) {
    NSData *data = [NSJSONSerialization dataWithJSONObject:object options:NSJSONWritingFragmentsAllowed error:nil];
    puts(data ? [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding].UTF8String : "{}");
    fflush(stdout);
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc < 5) {
            fprintf(stderr, "usage: permission-allow-state-c14 PID TITLE BUTTON inspect|move-button|move-out|down|drag-out|up [SNAPSHOT_PNG]\n");
            return 2;
        }
        if (!AXIsProcessTrusted()) {
            fprintf(stderr, "AX access is not already authorized; no permission prompt was requested\n");
            return 3;
        }
        NSString *mode = [NSString stringWithUTF8String:argv[4]];
        pid_t pid = (pid_t)strtol(argv[1], NULL, 10);
        NSString *title = [NSString stringWithUTF8String:argv[2]];
        NSString *buttonName = [NSString stringWithUTF8String:argv[3]];
        NSString *snapshotPath = argc > 5 ? [NSString stringWithUTF8String:argv[5]] : @"";
        AXUIElementRef app = AXUIElementCreateApplication(pid);
        CFTypeRef windowsValue = attribute(app, kAXWindowsAttribute);
        AXUIElementRef window = NULL;
        if (windowsValue && CFGetTypeID(windowsValue) == CFArrayGetTypeID()) {
            CFArrayRef windows = (CFArrayRef)windowsValue;
            for (CFIndex index = 0; index < CFArrayGetCount(windows); index++) {
                AXUIElementRef candidate = (AXUIElementRef)CFArrayGetValueAtIndex(windows, index);
                if (![stringAttribute(candidate, kAXRoleAttribute) isEqualToString:(__bridge NSString *)kAXWindowRole]) continue;
                AXUIElementRef match = findElement(candidate, NULL, title, 0);
                if (match) { CFRelease(match); window = (AXUIElementRef)CFRetain(candidate); break; }
            }
        }
        AXUIElementRef button = window ? findElement(window, kAXButtonRole, buttonName, 0) : NULL;
        CGRect windowRect = CGRectZero, buttonRect = CGRectZero;
        BOOL hasWindowRect = window && elementRect(window, &windowRect);
        BOOL hasButtonRect = button && elementRect(button, &buttonRect);
        if (!window || !button || !hasWindowRect || !hasButtonRect) {
            printObject(@{@"foundWindow": @(window != NULL), @"foundButton": @(button != NULL),
                          @"hasWindowRect": @(hasWindowRect), @"hasButtonRect": @(hasButtonRect)});
            if (button) CFRelease(button);
            if (window) CFRelease(window);
            if (windowsValue) CFRelease(windowsValue);
            if (app) CFRelease(app);
            return 4;
        }

        if ([mode isEqualToString:@"inspect"]) {
            BOOL enabledKnown = NO, enabled = NO, mainKnown = NO, main = NO;
            CFTypeRef enabledValue = attribute(button, kAXEnabledAttribute);
            if (enabledValue && CFGetTypeID(enabledValue) == CFBooleanGetTypeID()) {
                enabledKnown = YES; enabled = CFBooleanGetValue((CFBooleanRef)enabledValue);
            }
            if (enabledValue) CFRelease(enabledValue);
            CFTypeRef mainValue = attribute(window, kAXMainAttribute);
            if (mainValue && CFGetTypeID(mainValue) == CFBooleanGetTypeID()) {
                mainKnown = YES; main = CFBooleanGetValue((CFBooleanRef)mainValue);
            }
            if (mainValue) CFRelease(mainValue);
            NSDictionary *cg = windowInfo(pid, title);
            CGWindowID windowID = [cg[(id)kCGWindowNumber] unsignedIntValue];
            NSMutableDictionary *result = [@{
                @"foundWindow": @YES, @"foundButton": @YES,
                @"buttonEnabledKnown": @(enabledKnown), @"buttonEnabled": @(enabled),
                @"windowMainKnown": @(mainKnown), @"windowMain": @(main),
                @"appActive": @([NSRunningApplication runningApplicationWithProcessIdentifier:pid].isActive),
                @"windowFrameAX": rectObject(windowRect), @"buttonFrameAX": rectObject(buttonRect),
                @"cgWindowFound": @(cg != nil), @"cgWindowName": cg[(id)kCGWindowName] ?: @"",
                @"cgWindowBounds": cg[(id)kCGWindowBounds] ?: @{},
                @"eventPostAccess": @(CGPreflightPostEventAccess()),
            } mutableCopy];
            if (windowID && cg) [result addEntriesFromDictionary:pixelSample(windowID, cg, buttonRect, snapshotPath)];
            else result[@"captureError"] = @"On-screen host CGWindow row was not found";
            printObject(result);
        } else {
            CGPoint point = CGPointZero;
            CGEventType type = kCGEventNull;
            if ([mode isEqualToString:@"move-button"] || [mode isEqualToString:@"down"] || [mode isEqualToString:@"up-button"]) {
                point = CGPointMake(buttonRect.origin.x + buttonRect.size.width / 2, CGRectGetMidY(buttonRect));
                type = [mode isEqualToString:@"down"] ? kCGEventLeftMouseDown
                    : ([mode isEqualToString:@"up-button"] ? kCGEventLeftMouseUp : kCGEventMouseMoved);
            } else if ([mode isEqualToString:@"move-out"] || [mode isEqualToString:@"drag-out"] || [mode isEqualToString:@"up"]) {
                point = CGPointMake(windowRect.origin.x + 12, windowRect.origin.y + 12);
                type = [mode isEqualToString:@"move-out"] ? kCGEventMouseMoved
                    : ([mode isEqualToString:@"drag-out"] ? kCGEventLeftMouseDragged : kCGEventLeftMouseUp);
            } else {
                fprintf(stderr, "unknown mode: %s\n", mode.UTF8String);
                if (button) CFRelease(button);
                if (window) CFRelease(window);
                if (windowsValue) CFRelease(windowsValue);
                if (app) CFRelease(app);
                return 2;
            }
            BOOL eventPostAccess = CGPreflightPostEventAccess();
            BOOL posted = eventPostAccess && postMouse(type, point);
            printObject(@{@"mode": mode, @"posted": @(posted), @"eventPostAccess": @(eventPostAccess),
                          @"point": @{@"x": @(point.x), @"y": @(point.y)}});
            if (!posted) {
                if (button) CFRelease(button);
                if (window) CFRelease(window);
                if (windowsValue) CFRelease(windowsValue);
                if (app) CFRelease(app);
                return 5;
            }
        }

        if (button) CFRelease(button);
        if (window) CFRelease(window);
        if (windowsValue) CFRelease(windowsValue);
        if (app) CFRelease(app);
        return 0;
    }
}
