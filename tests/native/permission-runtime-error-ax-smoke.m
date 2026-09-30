#import <Cocoa/Cocoa.h>
#import <ApplicationServices/ApplicationServices.h>
#import <CoreGraphics/CoreGraphics.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static CFTypeRef axAttribute(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = NULL;
    if (element != NULL && AXUIElementCopyAttributeValue(element, name, &value) == kAXErrorSuccess) return value;
    return NULL;
}

static NSString *axString(AXUIElementRef element, CFStringRef name) {
    CFTypeRef value = axAttribute(element, name);
    if (value == NULL) return @"";
    NSString *result = CFGetTypeID(value) == CFStringGetTypeID()
        ? (__bridge NSString *)value
        : [(__bridge id)value description];
    NSString *copy = [result copy];
    CFRelease(value);
    return copy;
}

static BOOL axRect(AXUIElementRef element, CGRect *result) {
    CFTypeRef position = axAttribute(element, kAXPositionAttribute);
    CFTypeRef size = axAttribute(element, kAXSizeAttribute);
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

static NSString *normalized(NSString *value) {
    NSArray *parts = [value componentsSeparatedByCharactersInSet:NSCharacterSet.whitespaceAndNewlineCharacterSet];
    NSMutableArray *nonEmpty = [NSMutableArray array];
    for (NSString *part in parts) if (part.length) [nonEmpty addObject:part];
    return [nonEmpty componentsJoinedByString:@" "];
}

static BOOL axMatches(AXUIElementRef element, NSString *expected) {
    if (!expected.length) return YES;
    CFStringRef keys[] = { kAXDescriptionAttribute, kAXTitleAttribute, kAXValueAttribute };
    for (NSUInteger index = 0; index < sizeof(keys) / sizeof(keys[0]); index++) {
        NSString *actual = axString(element, keys[index]);
        if ([actual isEqualToString:expected] || [normalized(actual) isEqualToString:normalized(expected)]) return YES;
    }
    return NO;
}

static AXUIElementRef findAX(AXUIElementRef root, CFStringRef role, NSString *expected, int depth) {
    if (!root || depth > 32) return NULL;
    NSString *actualRole = axString(root, kAXRoleAttribute);
    BOOL roleMatches = !role || [actualRole isEqualToString:(__bridge NSString *)role];
    if (roleMatches && axMatches(root, expected)) return (AXUIElementRef)CFRetain(root);
    CFTypeRef childrenValue = axAttribute(root, kAXChildrenAttribute);
    if (childrenValue && CFGetTypeID(childrenValue) == CFArrayGetTypeID()) {
        CFArrayRef children = (CFArrayRef)childrenValue;
        for (CFIndex index = 0; index < CFArrayGetCount(children); index++) {
            AXUIElementRef child = (AXUIElementRef)CFArrayGetValueAtIndex(children, index);
            AXUIElementRef found = findAX(child, role, expected, depth + 1);
            if (found) { CFRelease(childrenValue); return found; }
        }
    }
    if (childrenValue) CFRelease(childrenValue);
    return NULL;
}

static AXUIElementRef findWindow(AXUIElementRef app, NSString *title, CFArrayRef *allWindows) {
    CFTypeRef value = axAttribute(app, kAXWindowsAttribute);
    if (!value || CFGetTypeID(value) != CFArrayGetTypeID()) {
        if (value) CFRelease(value);
        return NULL;
    }
    if (allWindows) *allWindows = (CFArrayRef)CFRetain(value);
    CFArrayRef windows = (CFArrayRef)value;
    AXUIElementRef found = NULL;
    for (CFIndex i = 0; i < CFArrayGetCount(windows); i++) {
        AXUIElementRef window = (AXUIElementRef)CFArrayGetValueAtIndex(windows, i);
        if ([axString(window, kAXRoleAttribute) isEqualToString:(__bridge NSString *)kAXWindowRole]) {
            AXUIElementRef match = findAX(window, NULL, title, 0);
            if (match) {
                CFRelease(match);
                found = (AXUIElementRef)CFRetain(window);
                break;
            }
        }
    }
    CFRelease(value);
    return found;
}

static NSArray<NSDictionary *> *cgWindows(pid_t pid) {
    CFArrayRef list = CGWindowListCopyWindowInfo(kCGWindowListOptionAll, kCGNullWindowID);
    if (!list) return @[];
    NSMutableArray *result = [NSMutableArray array];
    for (NSDictionary *entry in (__bridge NSArray *)list) {
        NSNumber *owner = entry[(id)kCGWindowOwnerPID];
        if (owner.intValue != pid) continue;
        [result addObject:@{
            @"name": entry[(id)kCGWindowName] ?: @"",
            @"layer": entry[(id)kCGWindowLayer] ?: @(-1),
            @"bounds": entry[(id)kCGWindowBounds] ?: @{},
            @"onscreen": entry[(id)kCGWindowIsOnscreen] ?: @NO,
        }];
    }
    CFRelease(list);
    return result;
}

static int inspect(pid_t pid, NSString *title, NSString *body, NSString *button, BOOL press) {
    AXUIElementRef app = AXUIElementCreateApplication(pid);
    CFArrayRef allWindows = NULL;
    AXUIElementRef window = findWindow(app, title, &allWindows);
    AXUIElementRef titleNode = window ? findAX(window, kAXStaticTextRole, title, 0) : NULL;
    AXUIElementRef bodyNode = window && body.length ? findAX(window, kAXStaticTextRole, body, 0) : NULL;
    AXUIElementRef buttonNode = window && button.length ? findAX(window, kAXButtonRole, button, 0) : NULL;
    CGRect frame = CGRectZero;
    BOOL hasFrame = window && axRect(window, &frame);
    BOOL enabledKnown = NO;
    BOOL enabled = YES;
    if (buttonNode) {
        CFTypeRef value = axAttribute(buttonNode, kAXEnabledAttribute);
        if (value && CFGetTypeID(value) == CFBooleanGetTypeID()) {
            enabled = CFBooleanGetValue((CFBooleanRef)value);
            enabledKnown = YES;
        }
        if (value) CFRelease(value);
    }
    AXError pressResult = kAXErrorNoValue;
    if (press && buttonNode) pressResult = AXUIElementPerformAction(buttonNode, kAXPressAction);
    NSArray *windows = cgWindows(pid);
    NSMutableArray *details = [NSMutableArray array];
    for (NSDictionary *item in windows) [details addObject:item];
    NSData *jsonData = [NSJSONSerialization dataWithJSONObject:details options:NSJSONWritingFragmentsAllowed error:nil];
    NSString *json = jsonData ? [[NSString alloc] initWithData:jsonData encoding:NSUTF8StringEncoding] : @"[]";
    NSUInteger onScreenCount = 0;
    for (NSDictionary *item in windows) if ([item[@"onscreen"] boolValue]) onScreenCount++;
    printf("{\"pid\":%d,\"axWindowCount\":%ld,\"cgWindowCount\":%lu,\"cgOnscreenWindowCount\":%lu,\"cgWindows\":%s,\"foundWindow\":%s,\"foundTitle\":%s,\"foundBody\":%s,\"foundButton\":%s,\"buttonEnabledKnown\":%s,\"buttonEnabled\":%s,\"height\":%.2f,\"width\":%.2f,\"pressResult\":%d}\n",
           pid, allWindows ? CFArrayGetCount(allWindows) : 0, (unsigned long)windows.count, (unsigned long)onScreenCount,
           json.UTF8String ?: "[]",
           window ? "true" : "false", titleNode ? "true" : "false", bodyNode ? "true" : "false",
           buttonNode ? "true" : "false", enabledKnown ? "true" : "false", enabled ? "true" : "false",
           hasFrame ? frame.size.height : 0, hasFrame ? frame.size.width : 0, pressResult);
    fflush(stdout);
    if (buttonNode) CFRelease(buttonNode);
    if (bodyNode) CFRelease(bodyNode);
    if (titleNode) CFRelease(titleNode);
    if (window) CFRelease(window);
    if (allWindows) CFRelease(allWindows);
    if (app) CFRelease(app);
    return 0;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc < 4) {
            fprintf(stderr, "usage: permission-runtime-error-ax-smoke PID TITLE BODY [BUTTON] [press]\n");
            return 2;
        }
        pid_t pid = (pid_t)strtol(argv[1], NULL, 10);
        NSString *title = [NSString stringWithUTF8String:argv[2]];
        NSString *body = [NSString stringWithUTF8String:argv[3]];
        NSString *button = argc > 4 ? [NSString stringWithUTF8String:argv[4]] : @"";
        BOOL press = argc > 5 && strcmp(argv[5], "press") == 0;
        return inspect(pid, title, body, button, press);
    }
}
