/*
 * [INPUT]: 依赖真实 renderer、临时.app、假NSUserDefaults域与命名图片资源。
 * [OUTPUT]: 验证官方选择镜像、主题资源、回退、路径边界与只读约束。
 * [POS]: Finder图标偏好隔离回归；不读或改真实Codex域，不启动App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#include "macos_launcher_icon.m"

static const uint8_t kHostColor[3] = {230, 30, 30};
static const uint8_t kChatGPTColor[3] = {30, 200, 200};
static const uint8_t kCodexLightColor[3] = {30, 210, 30};
static const uint8_t kCodexDarkColor[3] = {30, 30, 220};
static const uint8_t kSpaceLightColor[3] = {230, 200, 30};
static const uint8_t kSpaceDarkColor[3] = {210, 30, 190};
static const uint8_t kOutsideColor[3] = {240, 120, 20};
static const size_t kFixtureSize = 32;
static const size_t kOutputLimit = 64 * 1024 * 1024;

static void release_pixels(void *info, const void *data, size_t size) {
    (void)info;
    (void)size;
    free((void *)data);
}

static NSData *png_data(const uint8_t color[3], uint8_t alpha, size_t width, size_t height) {
    if (width == 0 || height == 0 || width > 16384 || height > 16384 ||
        width > SIZE_MAX / height / 4) return nil;
    size_t length = width * height * 4;
    uint8_t *pixels = calloc(length, 1);
    if (pixels == NULL) return nil;
    for (size_t offset = 0; offset < length; offset += 4) {
        pixels[offset] = color[0];
        pixels[offset + 1] = color[1];
        pixels[offset + 2] = color[2];
        pixels[offset + 3] = alpha;
    }

    CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGDataProviderRef provider = space == NULL ? NULL :
        CGDataProviderCreateWithData(NULL, pixels, length, release_pixels);
    CGImageRef image = provider == NULL ? NULL : CGImageCreate(width, height, 8, 32, width * 4,
        space, kCGImageAlphaLast | kCGBitmapByteOrder32Big, provider, NULL, false,
        kCGRenderingIntentDefault);
    if (provider == NULL) free(pixels);
    if (provider != NULL) CGDataProviderRelease(provider);
    if (space != NULL) CGColorSpaceRelease(space);
    if (image == NULL) return nil;

    CFMutableDataRef encoded = CFDataCreateMutable(kCFAllocatorDefault, 0);
    CGImageDestinationRef destination = encoded == NULL ? NULL :
        CGImageDestinationCreateWithData(encoded, CFSTR("public.png"), 1, NULL);
    BOOL ok = destination != NULL;
    if (ok) {
        CGImageDestinationAddImage(destination, image, NULL);
        ok = CGImageDestinationFinalize(destination);
    }
    if (destination != NULL) CFRelease(destination);
    CGImageRelease(image);
    if (!ok) {
        if (encoded != NULL) CFRelease(encoded);
        return nil;
    }
    NSData *result = [(__bridge NSData *)encoded copy];
    CFRelease(encoded);
    return result;
}

static NSData *icns_data(NSData *png) {
    if (png == nil || png.length > UINT32_MAX - 8) return nil;
    uint32_t chunkLength = CFSwapInt32HostToBig((uint32_t)png.length + 8);
    uint32_t fileLength = CFSwapInt32HostToBig((uint32_t)png.length + 16);
    NSMutableData *data = [NSMutableData dataWithBytes:"icns" length:4];
    [data appendBytes:&fileLength length:4];
    [data appendBytes:"icp5" length:4];
    [data appendBytes:&chunkLength length:4];
    [data appendData:png];
    return data;
}

static BOOL write_data(NSData *data, NSURL *url) {
    return data != nil && [data writeToURL:url options:NSDataWritingAtomic error:nil];
}

static NSDictionary *make_fixture(NSURL *root, NSString *name, NSString **bundleIDOut,
                                  NSURL **appOut, NSURL **resourcesOut) {
    NSFileManager *files = [NSFileManager defaultManager];
    NSURL *app = [root URLByAppendingPathComponent:[name stringByAppendingString:@".app"] isDirectory:YES];
    NSURL *contents = [app URLByAppendingPathComponent:@"Contents" isDirectory:YES];
    NSURL *resources = [contents URLByAppendingPathComponent:@"Resources" isDirectory:YES];
    NSURL *macOS = [contents URLByAppendingPathComponent:@"MacOS" isDirectory:YES];
    if (![files createDirectoryAtURL:resources withIntermediateDirectories:YES attributes:nil error:nil] ||
        ![files createDirectoryAtURL:macOS withIntermediateDirectories:YES attributes:nil error:nil]) return nil;

    NSData *hostPNG = png_data(kHostColor, 255, kFixtureSize, kFixtureSize);
    NSData *hostICNS = icns_data(hostPNG);
    NSData *chatGPTIcon = png_data(kChatGPTColor, 255, kFixtureSize, kFixtureSize);
    if (!write_data(hostICNS, [resources URLByAppendingPathComponent:@"Fixture.icns"]) ||
        !write_data(png_data(kChatGPTColor, 255, kFixtureSize, kFixtureSize),
                    [resources URLByAppendingPathComponent:@"icon-chatgpt.png"]) ||
        !write_data(icns_data(chatGPTIcon), [resources URLByAppendingPathComponent:@"icon-chatgpt.icns"]) ||
        !write_data(png_data(kCodexLightColor, 255, kFixtureSize, kFixtureSize),
                    [resources URLByAppendingPathComponent:@"icon-codex-light.png"]) ||
        !write_data(png_data(kCodexDarkColor, 255, kFixtureSize, kFixtureSize),
                    [resources URLByAppendingPathComponent:@"icon-codex-dark-color.png"]) ||
        !write_data(png_data(kSpaceLightColor, 255, kFixtureSize, kFixtureSize),
                    [resources URLByAppendingPathComponent:@"icon-space-light.png"]) ||
        !write_data(png_data(kSpaceDarkColor, 255, kFixtureSize, kFixtureSize),
                    [resources URLByAppendingPathComponent:@"icon-space-dark.png"]) ||
        !write_data([NSData dataWithBytes:"not a PNG" length:9],
                    [resources URLByAppendingPathComponent:@"icon-invalid.png"]) ||
        !write_data(png_data(kCodexLightColor, 255, kFixtureSize, kFixtureSize),
                    [resources URLByAppendingPathComponent:@"icon-text.txt"]) ||
        !write_data(png_data(kCodexLightColor, 255, 8193, 1),
                    [resources URLByAppendingPathComponent:@"icon-oversized.png"]) ||
        !write_data(png_data(kOutsideColor, 255, kFixtureSize, kFixtureSize),
                    [root URLByAppendingPathComponent:@"outside.png"])) return nil;

    if (![files createSymbolicLinkAtURL:[resources URLByAppendingPathComponent:@"icon-linked.png"]
                   withDestinationURL:[root URLByAppendingPathComponent:@"outside.png"] error:nil]) return nil;

    NSString *bundleID = [NSString stringWithFormat:@"com.incodex.fixture.launcher.%@.%@",
        name, [[[NSUUID UUID] UUIDString] lowercaseString]];
    NSDictionary *info = @{
        @"CFBundleIdentifier": bundleID,
        @"CFBundleName": @"Incodex Launcher Preference Fixture",
        @"CFBundlePackageType": @"APPL",
        @"CFBundleExecutable": @"fixture",
        @"CFBundleIconFile": @"Fixture.icns",
    };
    if (![info writeToURL:[contents URLByAppendingPathComponent:@"Info.plist"] atomically:YES]) return nil;

    NSURL *executable = [macOS URLByAppendingPathComponent:@"fixture"];
    if (!write_data([NSData dataWithBytes:"fixture; never launched" length:24], executable) ||
        ![files setAttributes:@{NSFilePosixPermissions: @0755} ofItemAtPath:executable.path error:nil]) return nil;

    if (bundleIDOut != NULL) *bundleIDOut = bundleID;
    if (appOut != NULL) *appOut = app;
    if (resourcesOut != NULL) *resourcesOut = resources;
    return info;
}

static NSDictionary *snapshot_app(NSURL *app) {
    NSFileManager *files = [NSFileManager defaultManager];
    NSDirectoryEnumerator *enumerator = [files enumeratorAtURL:app
        includingPropertiesForKeys:nil options:0 errorHandler:nil];
    if (enumerator == nil) return nil;
    NSMutableDictionary *snapshot = [NSMutableDictionary dictionary];
    for (NSURL *url in enumerator) {
        NSDictionary *attributes = [files attributesOfItemAtPath:url.path error:nil];
        NSString *value = nil;
        if ([attributes[NSFileType] isEqualToString:NSFileTypeSymbolicLink]) {
            value = [files destinationOfSymbolicLinkAtPath:url.path error:nil];
        } else if ([attributes[NSFileType] isEqualToString:NSFileTypeRegular]) {
            NSData *bytes = [NSData dataWithContentsOfURL:url];
            value = bytes == nil ? nil : [bytes base64EncodedStringWithOptions:0];
        } else {
            continue;
        }
        if (value == nil) return nil;
        NSString *relative = [url.path substringFromIndex:app.path.length + 1];
        snapshot[relative] = value;
    }
    return snapshot;
}

static BOOL output_color(const char *appPath, uint8_t color[3], char *error, size_t errorCapacity) {
    static const uint8_t transparent[3] = {0, 0, 0};
    NSData *badge = png_data(transparent, 0, 8, 8);
    if (badge == nil) {
        snprintf(error, errorCapacity, "cannot encode transparent badge fixture");
        return NO;
    }
    uint8_t *output = NULL;
    size_t outputLength = 0;
    int status = incodex_launcher_artwork_render(appPath, badge.bytes, badge.length,
        &output, &outputLength, error, errorCapacity);
    if (status != 1 || output == NULL || outputLength < 8 || outputLength > kOutputLimit) {
        if (output != NULL) free(output);
        if (error[0] == '\0') snprintf(error, errorCapacity, "renderer rejected a valid fixture app");
        return NO;
    }

    size_t offset = 8;
    CGImageRef image = NULL;
    while (offset + 8 <= outputLength) {
        uint32_t chunkLength = 0;
        memcpy(&chunkLength, output + offset + 4, sizeof(chunkLength));
        chunkLength = CFSwapInt32BigToHost(chunkLength);
        if (chunkLength < 8 || chunkLength > outputLength - offset) break;
        if (memcmp(output + offset, "ic10", 4) == 0) {
            CFDataRef png = CFDataCreate(kCFAllocatorDefault, output + offset + 8,
                                         (CFIndex)chunkLength - 8);
            CGImageSourceRef source = png == NULL ? NULL : CGImageSourceCreateWithData(png, NULL);
            image = source == NULL ? NULL : CGImageSourceCreateImageAtIndex(source, 0, NULL);
            if (source != NULL) CFRelease(source);
            if (png != NULL) CFRelease(png);
            break;
        }
        offset += chunkLength;
    }
    free(output);
    if (image == NULL) {
        snprintf(error, errorCapacity, "renderer output has no decodable 1024px ICNS image");
        return NO;
    }

    uint8_t pixel[4] = {0};
    CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    CGContextRef context = space == NULL ? NULL : CGBitmapContextCreate(pixel, 1, 1, 8, 4,
        space, kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
    if (context != NULL) CGContextDrawImage(context, CGRectMake(0, 0, 1, 1), image);
    if (context != NULL) CGContextRelease(context);
    if (space != NULL) CGColorSpaceRelease(space);
    CGImageRelease(image);
    if (context == NULL) {
        snprintf(error, errorCapacity, "cannot sample ICNS image");
        return NO;
    }
    memcpy(color, pixel, 3);
    return YES;
}

static BOOL close_color(const uint8_t actual[3], const uint8_t expected[3]) {
    for (size_t channel = 0; channel < 3; channel++) {
        int difference = (int)actual[channel] - (int)expected[channel];
        if (difference < -45 || difference > 45) return NO;
    }
    return YES;
}

static BOOL run_case(NSURL *root, NSString *name, id preference, NSString *resourceName,
                     const uint8_t expectedColor[3]) {
    NSString *bundleID = nil;
    NSURL *app = nil;
    NSURL *resources = nil;
    if (make_fixture(root, name, &bundleID, &app, &resources) == nil) {
        fprintf(stderr, "FAIL %s: cannot build fake application bundle\n", name.UTF8String);
        return NO;
    }
    NSBundle *fixtureBundle = [NSBundle bundleWithURL:app];
    if (fixtureBundle == nil || ![fixtureBundle.bundleIdentifier isEqualToString:bundleID]) {
        fprintf(stderr, "FAIL %s: fake bundle identifier is not available through NSBundle\n",
            name.UTF8String);
        return NO;
    }
    NSFileManager *files = [NSFileManager defaultManager];
    NSMutableDictionary *domain = [NSMutableDictionary dictionary];
    if (preference != nil) domain[@"DockIconPreference"] = preference;
    if (resourceName != nil) domain[@"DockIconResourceName"] = resourceName;
    NSUserDefaults *defaults = [NSUserDefaults standardUserDefaults];
    if (domain.count == 0) {
        [defaults removePersistentDomainForName:bundleID];
    } else {
        [defaults setPersistentDomain:domain forName:bundleID];
    }
    NSDictionary *beforeDomain = [defaults persistentDomainForName:bundleID] ?: @{};
    BOOL preferenceSetup = [beforeDomain isEqualToDictionary:domain];
    NSDictionary *beforeFiles = snapshot_app(app);
    NSData *infoBefore = [NSData dataWithContentsOfURL:[app URLByAppendingPathComponent:@"Contents/Info.plist"]];
    NSData *iconBefore = [NSData dataWithContentsOfURL:[resources URLByAppendingPathComponent:@"Fixture.icns"]];
    uint8_t actual[3] = {0};
    char error[512] = {0};
    BOOL rendered = output_color(app.path.UTF8String, actual, error, sizeof(error));
    NSDictionary *afterDomain = [defaults persistentDomainForName:bundleID] ?: @{};
    NSDictionary *afterFiles = snapshot_app(app);
    NSData *infoAfter = [NSData dataWithContentsOfURL:[app URLByAppendingPathComponent:@"Contents/Info.plist"]];
    NSData *iconAfter = [NSData dataWithContentsOfURL:[resources URLByAppendingPathComponent:@"Fixture.icns"]];

    BOOL preferenceReadOnly = [beforeDomain isEqualToDictionary:afterDomain];
    BOOL appReadOnly = [beforeFiles isEqualToDictionary:afterFiles] &&
        [infoBefore isEqualToData:infoAfter] && [iconBefore isEqualToData:iconAfter];
    BOOL selected = rendered && (expectedColor == NULL || close_color(actual, expectedColor));
    if (rendered && expectedColor == NULL) {
        [defaults removePersistentDomainForName:bundleID];
        uint8_t baseline[3] = {0};
        char baselineError[512] = {0};
        selected = output_color(app.path.UTF8String, baseline, baselineError, sizeof(baselineError)) &&
            close_color(actual, baseline);
        if (!selected) {
            fprintf(stderr, "FAIL %s: invalid value did not retain NSWorkspace fallback: %s\n",
                name.UTF8String, baselineError);
        }
    }
    if (!rendered || !selected || !preferenceSetup || !preferenceReadOnly || !appReadOnly) {
        fprintf(stderr, "FAIL %s: rendered=%d color=%u,%u,%u preferenceSetup=%d preferenceReadOnly=%d appReadOnly=%d error=%s\n",
            name.UTF8String, rendered, actual[0], actual[1], actual[2], preferenceSetup,
            preferenceReadOnly, appReadOnly, error);
    } else {
        printf("PASS %s: %u,%u,%u; app and preference domain unchanged\n",
            name.UTF8String, actual[0], actual[1], actual[2]);
    }
    [defaults removePersistentDomainForName:bundleID];
    [files removeItemAtURL:app error:nil];
    return rendered && selected && preferenceSetup && preferenceReadOnly && appReadOnly;
}

int main(int argc, const char *argv[]) {
    @autoreleasepool {
        if (argc != 3) {
            fprintf(stderr, "usage: preference-test TEMP_ROOT FAKE_HOME\n");
            return 2;
        }
        NSURL *root = [NSURL fileURLWithPath:[NSString stringWithUTF8String:argv[1]] isDirectory:YES];
        NSString *fakeHome = [NSString stringWithUTF8String:argv[2]];
        const char *processHome = getenv("HOME");
        if (processHome == NULL || ![fakeHome isEqualToString:[NSString stringWithUTF8String:processHome]]) {
            fprintf(stderr, "preference harness did not enter its fake HOME\n");
            return 3;
        }
        NSFileManager *files = [NSFileManager defaultManager];
        if (![files createDirectoryAtURL:root withIntermediateDirectories:YES attributes:nil error:nil]) {
            fprintf(stderr, "cannot create isolated preference test root\n");
            return 4;
        }

        BOOL ok = YES;
        ok &= run_case(root, @"chatgpt-app-default-png", @"app-default", @"icon-chatgpt.png", kChatGPTColor);
        ok &= run_case(root, @"chatgpt-app-default-icns", @"app-default", @"icon-chatgpt.icns", kChatGPTColor);
        ok &= run_case(root, @"codex-system-light", @"codex-system", @"icon-codex-light.png", kCodexLightColor);
        ok &= run_case(root, @"codex-system-dark", @"codex-system", @"icon-codex-dark-color.png", kCodexDarkColor);
        ok &= run_case(root, @"space-system-light", @"space-system", @"icon-space-light.png", kSpaceLightColor);
        ok &= run_case(root, @"space-system-dark", @"space-system", @"icon-space-dark.png", kSpaceDarkColor);
        ok &= run_case(root, @"legacy-codex-light", @"codex-light", @"icon-codex-light.png", kCodexLightColor);
        ok &= run_case(root, @"legacy-codex-dark", @"codex-dark", @"icon-codex-dark-color.png", kCodexDarkColor);
        ok &= run_case(root, @"missing-preference", nil, nil, NULL);
        ok &= run_case(root, @"unknown-preference", @"future-icon-mode", @"icon-codex-light.png", NULL);
        ok &= run_case(root, @"missing-resource-name", @"codex-system", nil, NULL);
        ok &= run_case(root, @"unknown-resource-name", @"codex-system", @"icon-missing.png", NULL);
        ok &= run_case(root, @"out-of-bundle-resource", @"codex-system", @"../../../outside.png", NULL);
        ok &= run_case(root, @"symlink-resource", @"space-system", @"icon-linked.png", NULL);
        ok &= run_case(root, @"unsupported-extension", @"codex-system", @"icon-text.txt", NULL);
        ok &= run_case(root, @"invalid-image-resource", @"codex-system", @"icon-invalid.png", NULL);
        ok &= run_case(root, @"oversized-image-resource", @"codex-system", @"icon-oversized.png", NULL);
        ok &= run_case(root, @"non-string-preference", @42, @"icon-codex-light.png", NULL);

        [files removeItemAtURL:root error:nil];
        return ok ? 0 : 1;
    }
}
