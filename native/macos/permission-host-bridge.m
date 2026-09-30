#import <Foundation/Foundation.h>
#import <CommonCrypto/CommonDigest.h>
#import <sys/stat.h>
#import <unistd.h>
#import <objc/message.h>

// Platform/ABI services only. Window composition and animation stay in the
// unchanged permission UI bundle and SwiftUI library.
id IncodexPermissionObjectFromPointer(void *pointer) { return (__bridge id)pointer; }

id IncodexPermissionDisplayBlock(id target) {
    return [^(double timestamp, double duration, double nextTimestamp) {
        ((void (*)(id, SEL, double, double, double))objc_msgSend)(
            target, @selector(tick:duration:target:), timestamp, duration, nextTimestamp);
    } copy];
}

void IncodexPermissionStartWindowClock(id clock, id window, id target) {
    ((void (*)(id, SEL, id, id))objc_msgSend)(clock, @selector(startForWindow:handler:), window, IncodexPermissionDisplayBlock(target));
}
void IncodexPermissionStartScreenClock(id clock, id screen, id target) {
    ((void (*)(id, SEL, id, id))objc_msgSend)(clock, @selector(startForScreen:handler:), screen, IncodexPermissionDisplayBlock(target));
}

@interface IncodexPermissionHostBridge : NSObject
+ (NSDictionary *)fileInfo:(NSString *)path;
+ (NSData *)readData:(NSString *)path;
+ (NSString *)sha256:(NSData *)data;
+ (NSNumber *)userID;
@end
@implementation IncodexPermissionHostBridge
+ (NSDictionary *)fileInfo:(NSString *)path {
    struct stat value;
    if (lstat(path.fileSystemRepresentation, &value) != 0) return nil;
    return @{ @"size": @(value.st_size), @"uid": @(value.st_uid), @"mode": @(value.st_mode),
              @"symlink": @(S_ISLNK(value.st_mode)), @"file": @(S_ISREG(value.st_mode)),
              @"directory": @(S_ISDIR(value.st_mode)) };
}
+ (NSData *)readData:(NSString *)path { return [NSData dataWithContentsOfFile:path]; }
+ (NSString *)sha256:(NSData *)data {
    if (!data || data.length > UINT32_MAX) return nil;
    unsigned char digest[CC_SHA256_DIGEST_LENGTH];
    CC_SHA256(data.bytes, (CC_LONG)data.length, digest);
    NSMutableString *result = [NSMutableString stringWithCapacity:64];
    for (NSUInteger i = 0; i < sizeof(digest); i++) [result appendFormat:@"%02x", digest[i]];
    return result;
}
+ (NSNumber *)userID { return @(getuid()); }
@end
