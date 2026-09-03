#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>

// 该入口先把 provider 固定为普通 Security.framework 客户端；后续 hook 红测会替换
// 这条直通实现。dylib 尚未接入安装事务，因此当前代码不会进入真实 ChatGPT 进程。
__attribute__((visibility("default")))
OSStatus incodex_key_provider_passthrough(CFDictionaryRef query,
                                          CFTypeRef *result) {
    return SecItemCopyMatching(query, result);
}
