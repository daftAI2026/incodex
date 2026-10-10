#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import "remote-key-policy.h"

static NSDictionary *request(void) {
 return @{(__bridge id)kSecAttrKeyType:(__bridge id)kSecAttrKeyTypeECSECPrimeRandom,
          (__bridge id)kSecAttrKeySizeInBits:@256,
          (__bridge id)kSecPrivateKeyAttrs:@{
           (__bridge id)kSecAttrApplicationTag:[@"com.openai.codex.device-key.test" dataUsingEncoding:NSUTF8StringEncoding],
           (__bridge id)kSecAttrIsPermanent:@YES,
           (__bridge id)kSecAttrIsExtractable:@NO,
           (__bridge id)kSecAttrAccessControl:(__bridge id)SecAccessControlCreateWithFlags(NULL,kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,kSecAccessControlPrivateKeyUsage,NULL)}};
}
int main(void) { @autoreleasepool {
 NSDictionary *p=request();
 NSCAssert(incodexRemoteKeyEligible((__bridge CFDictionaryRef)p), @"official OS fallback is eligible");
 NSMutableDictionary *hardware=[p mutableCopy];hardware[(__bridge id)kSecAttrTokenID]=(__bridge id)kSecAttrTokenIDSecureEnclave;
 NSCAssert(!incodexRemoteKeyEligible((__bridge CFDictionaryRef)hardware), @"never downgrade hardware policy");
 NSMutableDictionary *foreign=[p mutableCopy];NSMutableDictionary *attrs=[p[(__bridge id)kSecPrivateKeyAttrs] mutableCopy];
 attrs[(__bridge id)kSecAttrApplicationTag]=[@"unrelated-key" dataUsingEncoding:NSUTF8StringEncoding];foreign[(__bridge id)kSecPrivateKeyAttrs]=attrs;
 NSCAssert(!incodexRemoteKeyEligible((__bridge CFDictionaryRef)foreign), @"never touch unrelated keys");
 attrs[(__bridge id)kSecAttrApplicationTag]=@42;
 NSCAssert(!incodexRemoteKeyEligible((__bridge CFDictionaryRef)foreign), @"malformed tags fail closed");
 CFDictionaryRef retry=incodexRemoteKeyRetryAttributes((__bridge CFDictionaryRef)p);
 NSDictionary *r=CFBridgingRelease(retry);
 NSCAssert([r[(__bridge id)kSecUseDataProtectionKeychain] isEqual:@NO], @"explicit legacy keychain");
 NSCAssert([r[(__bridge id)kSecAttrIsExtractable] isEqual:@NO], @"top-level flag is essential for legacy keychain");
 NSCAssert([r[(__bridge id)kSecPrivateKeyAttrs] isEqual:p[(__bridge id)kSecPrivateKeyAttrs]], @"preserve original private attributes");
 NSCAssert(p[(__bridge id)kSecUseDataProtectionKeychain]==nil, @"do not mutate original request");
 puts("remote key policy contracts passed");
 } return 0; }
