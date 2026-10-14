/**
 * [INPUT]: 依赖Foundation与当前用户私有pending文件。
 * [OUTPUT]: 提供有界JSON、pending锁、交接身份与安装排队标记。
 * [POS]: interposer与Coordinator共用磁盘协议，不提供安装授权。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#ifndef INCODEX_MACOS_UPDATE_PENDING_H
#define INCODEX_MACOS_UPDATE_PENDING_H
#import <Foundation/Foundation.h>
#import <fcntl.h>
#import <sys/file.h>
#import <sys/stat.h>
#import <unistd.h>

static inline NSDictionary *incodexUpdateReadPrivateJSON(NSString *path, off_t maxBytes) {
    int descriptor = open(path.fileSystemRepresentation, O_RDONLY | O_NOFOLLOW);
    if (descriptor < 0) return nil;
    struct stat metadata;
    if (fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
        metadata.st_uid != geteuid() || (metadata.st_mode & 0077) != 0 ||
        metadata.st_size <= 0 || metadata.st_size > maxBytes) {
        close(descriptor);
        return nil;
    }
    NSFileHandle *handle = [[NSFileHandle alloc] initWithFileDescriptor:descriptor closeOnDealloc:YES];
    NSData *data = [handle readDataToEndOfFile];
    id body = data == nil ? nil : [NSJSONSerialization JSONObjectWithData:data options:0 error:nil];
    return [body isKindOfClass:NSDictionary.class] ? body : nil;
}

static inline BOOL incodexUpdateWritePrivateJSON(NSDictionary *body, NSString *path) {
    NSError *error = nil;
    NSData *data = [NSJSONSerialization dataWithJSONObject:body options:0 error:&error];
    if (data == nil || error != nil) return NO;
    if (![NSFileManager.defaultManager createDirectoryAtPath:path.stringByDeletingLastPathComponent
        withIntermediateDirectories:YES attributes:@{NSFilePosixPermissions: @0700} error:&error]) return NO;
    if (![data writeToFile:path options:NSDataWritingAtomic error:&error]) return NO;
    return chmod(path.fileSystemRepresentation, 0600) == 0;
}

static inline BOOL incodexUpdateWithPendingLock(NSString *pendingPath, BOOL (^body)(void)) {
    NSString *lockPath = [pendingPath.stringByDeletingLastPathComponent
        stringByAppendingPathComponent:@".pending.lock"];
    int descriptor = open(lockPath.fileSystemRepresentation, O_CREAT | O_RDWR | O_NOFOLLOW, 0600);
    if (descriptor < 0) return NO;
    struct stat metadata;
    BOOL valid = fstat(descriptor, &metadata) == 0 && S_ISREG(metadata.st_mode) &&
        metadata.st_uid == geteuid() && (metadata.st_mode & 0777) == 0600;
    if (!valid || flock(descriptor, LOCK_EX) != 0) {
        close(descriptor);
        return NO;
    }
    BOOL result = body();
    flock(descriptor, LOCK_UN);
    close(descriptor);
    return result;
}

static inline NSString *incodexUpdatePendingPathForCoordinator(NSString *bundlePath) {
    NSString *root = bundlePath;
    // App位于helpers/macos-update/SHA/App.app，向上四级回到Incodex root。
    for (NSUInteger index = 0; index < 4; index++) root = root.stringByDeletingLastPathComponent;
    return [root stringByAppendingPathComponent:@"macos-update/pending.json"];
}

static inline BOOL incodexUpdateSamePendingIdentity(NSDictionary *current, NSDictionary *expected) {
    for (NSString *key in @[@"installId", @"appPath", @"handoffId"]) {
        id value = current[key];
        if (![value isKindOfClass:NSString.class] || [value length] == 0 ||
            ![value isEqual:expected[key]]) return NO;
    }
    return YES;
}
// 回调已正常返回NO或YES都表示安装已排队；不拥有即时安装block。
static inline BOOL incodexUpdateMarkInstallQueued(NSString *pendingPath, NSDictionary *expected) {
    @try {
        return incodexUpdateWithPendingLock(pendingPath, ^BOOL{
            NSDictionary *current = incodexUpdateReadPrivateJSON(pendingPath, 64 * 1024);
            if (![current[@"schemaVersion"] isEqual:@2] ||
                !incodexUpdateSamePendingIdentity(current, expected) ||
                ![current[@"coordinatorPid"] isEqual:expected[@"coordinatorPid"]]) return NO;
            if ([current[@"installQueued"] isEqual:@YES]) return YES;
            NSMutableDictionary *next = current.mutableCopy;
            next[@"installQueued"] = @YES;
            return incodexUpdateWritePrivateJSON(next, pendingPath);
        });
    } @catch (__unused id exception) {
        // 自有交接写入失败不得改变官方回调的结果或异常。
        return NO;
    }
}
#endif
