/*
 * [INPUT]: 依赖生产图标合成器与上下、左右均非对称的测试图像。
 * [OUTPUT]: 验证 PNG 往返后源图方向和右下徽标位置均不变。
 * [POS]: artwork 渲染器隔离原生回归；仅由 Rust 测试临时编译。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#include "macos_launcher_icon.m"

static CGImageRef image_from_rgba_in_space(const uint8_t *pixels, size_t size,
                                            CGColorSpaceRef space) {
    CGDataProviderRef provider = CGDataProviderCreateWithData(NULL, pixels, size * size * 4, NULL);
    if (provider == NULL) return NULL;
    CGImageRef image = CGImageCreate(size, size, 8, 32, size * 4, space,
        kCGImageAlphaLast | kCGBitmapByteOrder32Big, provider, NULL, false, kCGRenderingIntentDefault);
    CGDataProviderRelease(provider);
    return image;
}

static CGImageRef image_from_rgba(const uint8_t *pixels, size_t size) {
    CGColorSpaceRef space = CGColorSpaceCreateWithName(kCGColorSpaceSRGB);
    if (space == NULL) return NULL;
    CGImageRef image = image_from_rgba_in_space(pixels, size, space);
    CGColorSpaceRelease(space);
    return image;
}

static CGImageRef png_round_trip(CGImageRef image) {
    CFMutableDataRef png = CFDataCreateMutable(kCFAllocatorDefault, 0);
    if (png == NULL) return NULL;
    CGImageDestinationRef destination = CGImageDestinationCreateWithData(png, CFSTR("public.png"), 1, NULL);
    if (destination == NULL) {
        CFRelease(png);
        return NULL;
    }
    CGImageDestinationAddImage(destination, image, NULL);
    BOOL encoded = CGImageDestinationFinalize(destination);
    CFRelease(destination);
    if (!encoded) {
        CFRelease(png);
        return NULL;
    }
    CGImageSourceRef source = CGImageSourceCreateWithData(png, NULL);
    CFRelease(png);
    if (source == NULL) return NULL;
    CGImageRef decoded = CGImageSourceCreateImageAtIndex(source, 0, NULL);
    CFRelease(source);
    return decoded;
}

static BOOL pixel_is(CGImageRef image, size_t x, size_t y, uint8_t red, uint8_t green,
                     uint8_t blue, uint8_t alpha) {
    CFDataRef data = CGDataProviderCopyData(CGImageGetDataProvider(image));
    if (data == NULL) return NO;
    size_t offset = y * CGImageGetBytesPerRow(image) + x * 4;
    const uint8_t *bytes = CFDataGetBytePtr(data);
    BOOL matches = offset + 4 <= (size_t)CFDataGetLength(data) &&
                   bytes[offset] == red && bytes[offset + 1] == green &&
                   bytes[offset + 2] == blue && bytes[offset + 3] == alpha;
    CFRelease(data);
    return matches;
}

static BOOL output_profile_is(CGImageRef badge, CFStringRef sourceName,
                             CFStringRef expectedName, char *error, size_t errorCapacity) {
    const size_t size = 64;
    uint8_t pixels[64 * 64 * 4];
    for (size_t offset = 0; offset < sizeof(pixels); offset += 4) {
        pixels[offset] = 90;
        pixels[offset + 1] = 150;
        pixels[offset + 2] = 210;
        pixels[offset + 3] = 255;
    }

    CGColorSpaceRef sourceSpace = CGColorSpaceCreateWithName(sourceName);
    if (sourceSpace == NULL) return NO;
    CGImageRef source = image_from_rgba_in_space(pixels, size, sourceSpace);
    CGColorSpaceRelease(sourceSpace);
    if (source == NULL) return NO;

    CGImageRef output = create_composite(source, badge, size, error, errorCapacity);
    CGImageRelease(source);
    if (output == NULL) return NO;
    CGColorSpaceRef outputSpace = CGImageGetColorSpace(output);
    CFStringRef actualName = outputSpace == NULL ? NULL : CGColorSpaceCopyName(outputSpace);
    BOOL matches = actualName != NULL && CFEqual(actualName, expectedName);
    if (actualName != NULL) CFRelease(actualName);
    CGImageRelease(output);
    return matches;
}

int main(void) {
    const size_t size = 64;
    uint8_t sourcePixels[64 * 64 * 4];
    uint8_t badgePixels[64 * 64 * 4];
    for (size_t y = 0; y < size; y++) {
        for (size_t x = 0; x < size; x++) {
            size_t offset = (y * size + x) * 4;
            BOOL top = y < size / 2;
            BOOL left = x < size / 2;
            const uint8_t source[4] = {
                top ? (left ? 255 : 0) : 255,
                top ? 0 : (left ? 255 : 255),
                left ? 0 : 255,
                255,
            };
            memcpy(&sourcePixels[offset], source, sizeof(source));
            memset(&badgePixels[offset], 0, 4);
            if (x >= 56 && y < 8) {
                badgePixels[offset] = 255;
                badgePixels[offset + 2] = 255;
                badgePixels[offset + 3] = 255;
            } else if (x < 8 && y >= 56) {
                badgePixels[offset + 1] = 255;
                badgePixels[offset + 2] = 255;
                badgePixels[offset + 3] = 255;
            } else if (x >= 56 && y >= 56) {
                badgePixels[offset + 1] = 255;
                badgePixels[offset + 3] = 255;
            }
        }
    }

    CGImageRef source = image_from_rgba(sourcePixels, size);
    CGImageRef badge = image_from_rgba(badgePixels, size);
    if (source == NULL || badge == NULL) {
        fprintf(stderr, "cannot create asymmetric source and badge fixtures\n");
        if (source != NULL) CGImageRelease(source);
        if (badge != NULL) CGImageRelease(badge);
        return 1;
    }
    if (!pixel_is(source, 8, 8, 255, 0, 0, 255) ||
        !pixel_is(source, 40, 8, 0, 0, 255, 255) ||
        !pixel_is(source, 8, 40, 255, 255, 0, 255) ||
        !pixel_is(source, 40, 40, 255, 255, 255, 255) ||
        !pixel_is(badge, 60, 4, 255, 0, 255, 255) ||
        !pixel_is(badge, 4, 60, 0, 255, 255, 255) ||
        !pixel_is(badge, 60, 60, 0, 255, 0, 255)) {
        fprintf(stderr, "asymmetric fixture pixel contract is invalid\n");
        CGImageRelease(source);
        CGImageRelease(badge);
        return 5;
    }

    char error[512] = {0};
    CGImageRef composed = create_composite(source, badge, size, error, sizeof(error));
    if (composed == NULL) {
        fprintf(stderr, "cannot composite orientation fixture: %s\n", error);
        CGImageRelease(source);
        CGImageRelease(badge);
        return 2;
    }
    CGImageRef output = png_round_trip(composed);
    if (output == NULL) {
        fprintf(stderr, "cannot decode composed PNG fixture\n");
        CGImageRelease(composed);
        CGImageRelease(source);
        CGImageRelease(badge);
        return 3;
    }

    BOOL sourceKeptOrientation = pixel_is(output, 8, 8, 255, 0, 0, 255) &&
                                 pixel_is(output, 40, 8, 0, 0, 255, 255) &&
                                 pixel_is(output, 8, 40, 255, 255, 0, 255) &&
                                 pixel_is(output, 40, 40, 255, 255, 255, 255);
    BOOL badgeKeptOrientation = pixel_is(output, 60, 4, 255, 0, 255, 255) &&
                                pixel_is(output, 4, 60, 0, 255, 255, 255) &&
                                pixel_is(output, 60, 60, 0, 255, 0, 255);
    CGImageRelease(output);
    CGImageRelease(composed);
    CGImageRelease(source);
    CGImageRelease(badge);

    CGImageRef profileBadge = image_from_rgba(badgePixels, size);
    if (profileBadge == NULL) {
        fprintf(stderr, "cannot create profile badge fixture\n");
        return 6;
    }
    char profileError[512] = {0};
    BOOL extendedMapsToSrgb = output_profile_is(profileBadge, kCGColorSpaceExtendedSRGB,
                                                 kCGColorSpaceSRGB,
                                                 profileError, sizeof(profileError));
    BOOL displayP3StaysP3 = output_profile_is(profileBadge, kCGColorSpaceDisplayP3,
                                               kCGColorSpaceDisplayP3,
                                               profileError, sizeof(profileError));
    CGImageRelease(profileBadge);
    if (!extendedMapsToSrgb || !displayP3StaysP3) {
        fprintf(stderr, "source profile contract failed: extended=%d p3=%d %s\n",
                extendedMapsToSrgb, displayP3StaysP3, profileError);
        return 7;
    }

    if (!sourceKeptOrientation || !badgeKeptOrientation) {
        fprintf(stderr, "PNG orientation mismatch: source=%d badge=%d\n",
                sourceKeptOrientation, badgeKeptOrientation);
        return 4;
    }
    return 0;
}
