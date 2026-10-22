/*
 * [INPUT]: 依赖目标.bundle ID的官方图标镜像、包内选定PNG/ICNS、LaunchServices回退与透明徽标。
 * [OUTPUT]: 对外提供带徽标的16至1024像素ICNS字节。
 * [POS]: CLI Finder图标渲染桥；只读目标偏好，安全资源优先，失败回退LaunchServices。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <ImageIO/ImageIO.h>
#import <Foundation/Foundation.h>

#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { kLauncherIconSizeCount = 7 };
static const size_t kLauncherIconSizes[kLauncherIconSizeCount] = {16, 32, 64, 128, 256, 512, 1024};
static const size_t kMaximumBadgeBytes = 16 * 1024 * 1024;
static const size_t kMaximumIconBytes = 64 * 1024 * 1024;
static const char *kLauncherIconTags[] = {"icp4", "icp5", "icp6", "ic07", "ic08", "ic09", "ic10"};

static int fail(char *buffer, size_t capacity, const char *format, ...) {
    if (buffer != NULL && capacity > 0) {
        va_list args;
        va_start(args, format);
        vsnprintf(buffer, capacity, format, args);
        va_end(args);
        buffer[capacity - 1] = '\0';
    }
    return 0;
}

static BOOL has_declared_icon(NSString *path, NSDictionary *info) {
    NSString *resources = [path stringByAppendingPathComponent:@"Contents/Resources"];
    NSString *iconFile = [info[@"CFBundleIconFile"] isKindOfClass:[NSString class]] ? info[@"CFBundleIconFile"] : nil;
    if (iconFile.length > 0 && ![iconFile containsString:@"/"] && ![iconFile containsString:@"\\"] &&
        ![iconFile isEqualToString:@"."] && ![iconFile isEqualToString:@".."] ) {
        NSString *declaredPath = [resources stringByAppendingPathComponent:iconFile];
        if ([[NSFileManager defaultManager] isReadableFileAtPath:declaredPath]) return YES;
        if ([iconFile pathExtension].length == 0) {
            for (NSString *extension in @[@"icns", @"png", @"tiff"]) {
                NSString *candidate = [[resources stringByAppendingPathComponent:iconFile] stringByAppendingPathExtension:extension];
                if ([[NSFileManager defaultManager] isReadableFileAtPath:candidate]) return YES;
            }
        }
    }

    NSString *iconName = [info[@"CFBundleIconName"] isKindOfClass:[NSString class]] ? info[@"CFBundleIconName"] : nil;
    NSDictionary *iconDictionary = [info[@"CFBundleIcons"] isKindOfClass:[NSDictionary class]] ? info[@"CFBundleIcons"] : nil;
    NSString *assetsCatalog = [resources stringByAppendingPathComponent:@"Assets.car"];
    return (iconName.length > 0 || iconDictionary != nil) &&
           [[NSFileManager defaultManager] isReadableFileAtPath:assetsCatalog];
}

static BOOL validate_app_bundle(NSString *path, char *error, size_t errorCapacity) {
    if ([[path pathExtension] caseInsensitiveCompare:@"app"] != NSOrderedSame) {
        return fail(error, errorCapacity, "resolved source path is not an .app bundle");
    }

    BOOL isDirectory = NO;
    if (![[NSFileManager defaultManager] fileExistsAtPath:path isDirectory:&isDirectory] || !isDirectory) {
        return fail(error, errorCapacity, "resolved .app path is not a directory");
    }

    NSString *infoPath = [path stringByAppendingPathComponent:@"Contents/Info.plist"];
    NSDictionary *info = [NSDictionary dictionaryWithContentsOfFile:infoPath];
    NSString *bundleID = [info isKindOfClass:[NSDictionary class]] ? info[@"CFBundleIdentifier"] : nil;
    NSString *packageType = [info isKindOfClass:[NSDictionary class]] ? info[@"CFBundlePackageType"] : nil;
    NSString *executable = [info isKindOfClass:[NSDictionary class]] ? info[@"CFBundleExecutable"] : nil;
    if (![bundleID isKindOfClass:[NSString class]] || bundleID.length == 0 ||
        ![packageType isKindOfClass:[NSString class]] || ![packageType isEqualToString:@"APPL"] ||
        ![executable isKindOfClass:[NSString class]] || executable.length == 0 ||
        [executable containsString:@"/"] || [executable isEqualToString:@"."] || [executable isEqualToString:@".."] ) {
        return fail(error, errorCapacity, "source is not a valid application bundle");
    }
    if (!has_declared_icon(path, info)) {
        return fail(error, errorCapacity, "application bundle has no declared readable icon resource");
    }

    NSString *executablePath = [[path stringByAppendingPathComponent:@"Contents/MacOS"] stringByAppendingPathComponent:executable];
    if (![[NSFileManager defaultManager] isExecutableFileAtPath:executablePath]) {
        return fail(error, errorCapacity, "application bundle executable is missing or not executable");
    }
    return YES;
}

static CGImageRef decode_badge(const uint8_t *bytes, size_t length, char *error, size_t errorCapacity) {
    if (bytes == NULL || length == 0 || length > kMaximumBadgeBytes) {
        fail(error, errorCapacity, "badge PNG length is invalid");
        return NULL;
    }
    CFDataRef data = CFDataCreate(kCFAllocatorDefault, bytes, (CFIndex)length);
    if (data == NULL) {
        fail(error, errorCapacity, "cannot allocate badge data");
        return NULL;
    }
    CGImageSourceRef source = CGImageSourceCreateWithData(data, NULL);
    CFRelease(data);
    if (source == NULL) {
        fail(error, errorCapacity, "cannot decode badge PNG");
        return NULL;
    }

    CFStringRef type = CGImageSourceGetType(source);
    if (type == NULL || !CFEqual(type, CFSTR("public.png"))) {
        CFRelease(source);
        fail(error, errorCapacity, "badge asset must be a PNG image");
        return NULL;
    }
    CGImageRef image = CGImageSourceCreateImageAtIndex(source, 0, NULL);
    CFRelease(source);
    if (image == NULL) {
        fail(error, errorCapacity, "cannot create badge image");
        return NULL;
    }

    size_t width = CGImageGetWidth(image);
    size_t height = CGImageGetHeight(image);
    CGImageAlphaInfo alpha = CGImageGetAlphaInfo(image);
    BOOL hasAlpha = alpha == kCGImageAlphaPremultipliedLast || alpha == kCGImageAlphaPremultipliedFirst ||
                    alpha == kCGImageAlphaLast || alpha == kCGImageAlphaFirst;
    if (width == 0 || width != height || width > 4096 || !hasAlpha) {
        CGImageRelease(image);
        fail(error, errorCapacity, "badge must be a square PNG with transparency");
        return NULL;
    }
    return image;
}

static CGImageRef create_composite(CGImageRef source, CGImageRef badge, size_t size, char *error, size_t errorCapacity) {
    size_t sourceWidth = CGImageGetWidth(source);
    size_t sourceHeight = CGImageGetHeight(source);
    if (sourceWidth == 0 || sourceHeight == 0 || sourceWidth > 8192 || sourceHeight > 8192) {
        fail(error, errorCapacity, "resolved application icon has invalid dimensions");
        return NULL;
    }

    CGColorSpaceRef sourceSpace = CGImageGetColorSpace(source);
    if (sourceSpace == NULL || CGColorSpaceGetModel(sourceSpace) != kCGColorSpaceModelRGB) {
        fail(error, errorCapacity, "resolved application icon has no RGB color profile");
        return NULL;
    }
    CFStringRef sourceName = CGColorSpaceCopyName(sourceSpace);
    BOOL extendedSRGB = sourceName != NULL && CFEqual(sourceName, kCGColorSpaceExtendedSRGB);
    if (sourceName != NULL) CFRelease(sourceName);

    // ExtendedSRGB 不支持本路径的 8 位目标。先映射，避免失败创建刷系统警告。
    CGColorSpaceRef mappedSpace = extendedSRGB ? CGColorSpaceCreateWithName(kCGColorSpaceSRGB) : NULL;
    if (extendedSRGB && mappedSpace == NULL) {
        fail(error, errorCapacity, "cannot create standard sRGB fallback for extended sRGB icon");
        return NULL;
    }
    CGColorSpaceRef bitmapSpace = mappedSpace == NULL ? sourceSpace : mappedSpace;
    CGContextRef context = CGBitmapContextCreate(NULL, size, size, 8, size * 4, bitmapSpace,
        kCGImageAlphaPremultipliedLast | kCGBitmapByteOrder32Big);
    if (mappedSpace != NULL) CGColorSpaceRelease(mappedSpace);
    if (context == NULL) {
        fail(error, errorCapacity, extendedSRGB
            ? "cannot allocate bounded sRGB icon bitmap"
            : "cannot allocate icon bitmap in its source color profile");
        return NULL;
    }

    CGContextClearRect(context, CGRectMake(0, 0, size, size));
    CGContextSetInterpolationQuality(context, kCGInterpolationHigh);
    CGContextSetBlendMode(context, kCGBlendModeNormal);

    CGFloat scale = MIN((CGFloat)size / sourceWidth, (CGFloat)size / sourceHeight);
    CGFloat width = sourceWidth * scale;
    CGFloat height = sourceHeight * scale;
    CGRect sourceRect = CGRectMake(((CGFloat)size - width) / 2.0, ((CGFloat)size - height) / 2.0, width, height);

    // 两个 CGImage 共用默认 CTM，保持各自像素方向。
    CGContextDrawImage(context, sourceRect, source);
    CGContextDrawImage(context, CGRectMake(0, 0, size, size), badge);

    CGImageRef result = CGBitmapContextCreateImage(context);
    CGContextRelease(context);
    if (result == NULL) fail(error, errorCapacity, "cannot finalize composed icon image");
    return result;
}

static NSString *canonical_existing_path(NSString *path) {
    if (path.length == 0) return nil;
    char *resolved = realpath(path.fileSystemRepresentation, NULL);
    if (resolved == NULL) return nil;
    NSString *canonical = [NSString stringWithUTF8String:resolved];
    free(resolved);
    return canonical;
}

static BOOL is_child_path(NSString *parent, NSString *child) {
    if (parent.length == 0 || child.length == 0 || [parent isEqualToString:child]) return NO;
    NSString *prefix = [parent hasSuffix:@"/"] ? parent : [parent stringByAppendingString:@"/"];
    return [child hasPrefix:prefix];
}

static CGImageRef decode_bundle_icon_resource(NSString *resolvedPath, NSString *extension) {
    NSDictionary *attributes = [[NSFileManager defaultManager] attributesOfItemAtPath:resolvedPath error:nil];
    if (![attributes[NSFileType] isEqualToString:NSFileTypeRegular]) return NULL;
    unsigned long long length = [attributes[NSFileSize] unsignedLongLongValue];
    if (length == 0 || length > kMaximumIconBytes) return NULL;

    CFURLRef url = CFURLCreateWithFileSystemPath(kCFAllocatorDefault, (__bridge CFStringRef)resolvedPath,
        kCFURLPOSIXPathStyle, false);
    CGImageSourceRef source = url == NULL ? NULL : CGImageSourceCreateWithURL(url, NULL);
    if (url != NULL) CFRelease(url);
    if (source == NULL) return NULL;

    CFStringRef type = CGImageSourceGetType(source);
    BOOL png = [extension isEqualToString:@"png"] && type != NULL && CFEqual(type, CFSTR("public.png"));
    BOOL icns = [extension isEqualToString:@"icns"] && type != NULL &&
        (CFEqual(type, CFSTR("com.apple.icns")) || CFEqual(type, CFSTR("public.icns")));
    size_t count = CGImageSourceGetCount(source);
    if ((!png && !icns) || count == 0 || (png && count != 1)) {
        CFRelease(source);
        return NULL;
    }

    size_t bestIndex = 0;
    uint64_t bestArea = 0;
    for (size_t index = 0; index < count; index++) {
        CFDictionaryRef properties = CGImageSourceCopyPropertiesAtIndex(source, index, NULL);
        if (properties == NULL) {
            CFRelease(source);
            return NULL;
        }
        CFNumberRef widthValue = CFDictionaryGetValue(properties, kCGImagePropertyPixelWidth);
        CFNumberRef heightValue = CFDictionaryGetValue(properties, kCGImagePropertyPixelHeight);
        int64_t width = 0;
        int64_t height = 0;
        BOOL valid = widthValue != NULL && heightValue != NULL &&
            CFGetTypeID(widthValue) == CFNumberGetTypeID() &&
            CFGetTypeID(heightValue) == CFNumberGetTypeID() &&
            CFNumberGetValue(widthValue, kCFNumberSInt64Type, &width) &&
            CFNumberGetValue(heightValue, kCFNumberSInt64Type, &height) &&
            width > 0 && height > 0 && width <= 8192 && height <= 8192;
        CFRelease(properties);
        if (!valid) {
            CFRelease(source);
            return NULL;
        }
        uint64_t area = (uint64_t)width * (uint64_t)height;
        if (area > bestArea) {
            bestArea = area;
            bestIndex = index;
        }
    }

    CGImageRef image = CGImageSourceCreateImageAtIndex(source, bestIndex, NULL);
    CFRelease(source);
    if (image == NULL) return NULL;
    size_t width = CGImageGetWidth(image);
    size_t height = CGImageGetHeight(image);
    CGColorSpaceRef colorSpace = CGImageGetColorSpace(image);
    if (width == 0 || height == 0 || width > 8192 || height > 8192 ||
        colorSpace == NULL || CGColorSpaceGetModel(colorSpace) != kCGColorSpaceModelRGB) {
        CGImageRelease(image);
        return NULL;
    }
    return image;
}

static id copy_bundle_preference(NSString *bundleID, CFStringRef key) {
    CFPropertyListRef value = CFPreferencesCopyValue(key, (__bridge CFStringRef)bundleID,
        kCFPreferencesCurrentUser, kCFPreferencesAnyHost);
    return value == NULL ? nil : CFBridgingRelease(value);
}

static CGImageRef selected_bundle_icon_image(NSString *appPath) {
    NSURL *appURL = [NSURL fileURLWithPath:appPath isDirectory:YES];
    NSBundle *bundle = [NSBundle bundleWithURL:appURL];
    NSString *bundleID = bundle.bundleIdentifier;
    if (bundleID.length == 0) return NULL;

    id preferenceValue = copy_bundle_preference(bundleID, CFSTR("DockIconPreference"));
    if (![preferenceValue isKindOfClass:[NSString class]]) return NULL;
    NSString *preference = (NSString *)preferenceValue;
    if ([preference isEqualToString:@"codex-light"] || [preference isEqualToString:@"codex-dark"]) {
        preference = @"codex-system";
    }
    if (![preference isEqualToString:@"app-default"] &&
        ![preference isEqualToString:@"codex-system"] &&
        ![preference isEqualToString:@"space-system"]) {
        return NULL;
    }

    id resourceValue = copy_bundle_preference(bundleID, CFSTR("DockIconResourceName"));
    if (![resourceValue isKindOfClass:[NSString class]]) return NULL;
    NSString *resourceName = (NSString *)resourceValue;
    unichar nul = 0;
    NSString *nulString = [NSString stringWithCharacters:&nul length:1];
    if (resourceName.length == 0 ||
        [resourceName containsString:@"/"] || [resourceName containsString:@"\\"] ||
        [resourceName containsString:nulString] ||
        ![[resourceName lastPathComponent] isEqualToString:resourceName] ||
        [resourceName isEqualToString:@"."] || [resourceName isEqualToString:@".."] ) {
        return NULL;
    }

    NSString *extension = [[resourceName pathExtension] lowercaseString];
    if (![extension isEqualToString:@"png"] && ![extension isEqualToString:@"icns"]) return NULL;
    NSURL *resourceURL = bundle.resourceURL;
    if (resourceURL == nil) return NULL;
    NSURL *candidateURL = [resourceURL URLByAppendingPathComponent:resourceName isDirectory:NO];
    NSString *canonicalApp = canonical_existing_path(appPath);
    NSString *canonicalResources = canonical_existing_path(resourceURL.path);
    NSString *canonicalCandidate = canonical_existing_path(candidateURL.path);
    if (!is_child_path(canonicalApp, canonicalResources) ||
        !is_child_path(canonicalResources, canonicalCandidate)) return NULL;
    return decode_bundle_icon_resource(canonicalCandidate, extension);
}

static BOOL append_png_representation(CFMutableDataRef output, const char *tag, CGImageRef image,
                                      char *error, size_t errorCapacity) {
    CFMutableDataRef png = CFDataCreateMutable(kCFAllocatorDefault, 0);
    if (png == NULL) return fail(error, errorCapacity, "cannot allocate ICNS representation");
    CGImageDestinationRef destination = CGImageDestinationCreateWithData(png, CFSTR("public.png"), 1, NULL);
    if (destination == NULL) {
        CFRelease(png);
        return fail(error, errorCapacity, "ImageIO cannot encode ICNS PNG representation");
    }
    CGImageDestinationAddImage(destination, image, NULL);
    BOOL ok = CGImageDestinationFinalize(destination);
    CFRelease(destination);
    CFIndex pngLength = CFDataGetLength(png);
    size_t outputLength = (size_t)CFDataGetLength(output);
    if (!ok || pngLength <= 0 || (uint64_t)pngLength > UINT32_MAX - 8 ||
        outputLength > kMaximumIconBytes - 8 || (size_t)pngLength > kMaximumIconBytes - outputLength - 8) {
        CFRelease(png);
        return fail(error, errorCapacity, "ImageIO failed to encode bounded ICNS PNG representation");
    }
    UInt32 chunkLength = CFSwapInt32HostToBig((UInt32)pngLength + 8);
    CFDataAppendBytes(output, (const UInt8 *)tag, 4);
    CFDataAppendBytes(output, (const UInt8 *)&chunkLength, 4);
    CFDataAppendBytes(output, CFDataGetBytePtr(png), pngLength);
    CFRelease(png);
    if ((size_t)CFDataGetLength(output) > kMaximumIconBytes) {
        return fail(error, errorCapacity, "composed ICNS exceeds its size limit");
    }
    return YES;
}

static CFDataRef create_icns_from_source(CGImageRef source, CGImageRef badge,
                                         char *error, size_t errorCapacity) {
    CFMutableDataRef data = CFDataCreateMutable(kCFAllocatorDefault, 0);
    if (data == NULL) {
        fail(error, errorCapacity, "cannot allocate ICNS output");
        return NULL;
    }
    CFDataAppendBytes(data, (const UInt8 *)"icns", 4);
    UInt32 placeholder = 0;
    CFDataAppendBytes(data, (const UInt8 *)&placeholder, 4);
    for (size_t index = 0; index < kLauncherIconSizeCount; index++) {
        CGImageRef image = create_composite(source, badge, kLauncherIconSizes[index], error, errorCapacity);
        if (image == NULL) {
            CFRelease(data);
            return NULL;
        }
        BOOL ok = append_png_representation(data, kLauncherIconTags[index], image, error, errorCapacity);
        CGImageRelease(image);
        if (!ok) {
            CFRelease(data);
            return NULL;
        }
    }
    size_t outputLength = (size_t)CFDataGetLength(data);
    if (outputLength < 8 || outputLength > kMaximumIconBytes || outputLength > UINT32_MAX) {
        CFRelease(data);
        fail(error, errorCapacity, "composed ICNS exceeds its size limit");
        return NULL;
    }
    UInt32 encodedLength = CFSwapInt32HostToBig((UInt32)outputLength);
    CFDataReplaceBytes(data, CFRangeMake(4, 4), (const UInt8 *)&encodedLength, 4);
    return data;
}

static CFDataRef create_icns(NSImage *hostIcon, CGImageRef badge, char *error, size_t errorCapacity) {
    CGRect proposed = CGRectMake(0, 0, 1024, 1024);
    CGImageRef source = [hostIcon CGImageForProposedRect:&proposed context:nil hints:nil];
    if (source == NULL) {
        fail(error, errorCapacity, "Launch Services did not provide a usable application icon");
        return NULL;
    }
    return create_icns_from_source(source, badge, error, errorCapacity);
}

int incodex_launcher_artwork_render(const char *appPath, const uint8_t *badgeBytes, size_t badgeLength,
                                    uint8_t **outBytes, size_t *outLength, char *error, size_t errorCapacity) {
    if (outBytes != NULL) *outBytes = NULL;
    if (outLength != NULL) *outLength = 0;
    if (error != NULL && errorCapacity > 0) error[0] = '\0';
    if (appPath == NULL || outBytes == NULL || outLength == NULL) {
        return fail(error, errorCapacity, "invalid launcher artwork arguments");
    }

    @autoreleasepool {
        NSString *path = [NSString stringWithUTF8String:appPath];
        if (path == nil || !validate_app_bundle(path, error, errorCapacity)) return 0;

        CGImageRef badge = decode_badge(badgeBytes, badgeLength, error, errorCapacity);
        if (badge == NULL) return 0;

        // 只读所选.bundle镜像；未识别或不安全时保留Launch Services回退。
        CGImageRef selectedIcon = selected_bundle_icon_image(path);
        CFDataRef icns = NULL;
        if (selectedIcon != NULL) {
            char ignoredResourceError[512] = {0};
            icns = create_icns_from_source(selectedIcon, badge, ignoredResourceError,
                                           sizeof(ignoredResourceError));
            CGImageRelease(selectedIcon);
        }
        if (icns == NULL) {
            // 使用Launch Services图标回退；不以通用文件图标绕过bundle校验。
            NSImage *hostIcon = [[NSWorkspace sharedWorkspace] iconForFile:path];
            if (hostIcon == nil || hostIcon.size.width <= 0 || hostIcon.size.height <= 0) {
                CGImageRelease(badge);
                return fail(error, errorCapacity, "Launch Services could not resolve the application icon");
            }
            icns = create_icns(hostIcon, badge, error, errorCapacity);
        }
        CGImageRelease(badge);
        if (icns == NULL) return 0;

        CFIndex length = CFDataGetLength(icns);
        uint8_t *copy = (uint8_t *)malloc((size_t)length);
        if (copy == NULL) {
            CFRelease(icns);
            return fail(error, errorCapacity, "cannot allocate returned ICNS bytes");
        }
        memcpy(copy, CFDataGetBytePtr(icns), (size_t)length);
        CFRelease(icns);
        *outBytes = copy;
        *outLength = (size_t)length;
        return 1;
    }
}
