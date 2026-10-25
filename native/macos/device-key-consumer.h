/**
 * [INPUT]: 依赖闭域 JSON 请求与调用方提供的 classic Keychain 搜索范围。
 * [OUTPUT]: 返回官方公开密钥或受限签名结果，并区分协议与 Security 错误。
 * [POS]: 短命 device-key helper 的纯消费接口；不创建、删除或导出私钥。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#pragma once
#import <Foundation/Foundation.h>
#import <Security/Security.h>

typedef NS_ENUM(NSInteger, IncodexDeviceKeyConsumerOutcome) {
 IncodexDeviceKeyConsumerOutcomeProtocolError,
 IncodexDeviceKeyConsumerOutcomeSecurityError,
 IncodexDeviceKeyConsumerOutcomeSuccess,
};

NSDictionary *incodexDeviceKeyConsumeRequest(NSDictionary *request, SecKeychainRef keychain,
                                              IncodexDeviceKeyConsumerOutcome *outcome,
                                              OSStatus *securityStatus);

#if defined(INCODEX_DEVICE_KEY_CONSUMER_TESTING)
OSStatus incodexDeviceKeyTestItemCopyMatching(CFDictionaryRef query, CFTypeRef *result);
#endif

#if defined(INCODEX_DEVICE_KEY_LIFECYCLE_TESTING)
#import <dispatch/dispatch.h>
#import <sys/proc_info.h>
dispatch_source_t incodexDeviceKeyTestWatchParentExit(pid_t parentPID,
  const struct proc_bsdinfo *expectedParent, const char *expectedPath);
#endif
