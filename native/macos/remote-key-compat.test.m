#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import "remote-key-policy.h"

static NSDictionary *request(void) {
 return @{(__bridge id)kSecAttrLabel:@"Official device key fixture",
          (__bridge id)kSecAttrKeyType:(__bridge id)kSecAttrKeyTypeECSECPrimeRandom,
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
 // Exercise the real Security API in a disposable, uniquely owned keychain.
 NSString *chainPath=[NSTemporaryDirectory() stringByAppendingPathComponent:[@"incodex-key-test-" stringByAppendingString:NSUUID.UUID.UUIDString]];
 SecKeychainRef chain=NULL;
 NSCAssert(SecKeychainCreate(chainPath.UTF8String,8,"testpass",false,NULL,&chain)==errSecSuccess,@"create synthetic keychain");
 NSMutableDictionary *actual=[r mutableCopy];actual[(__bridge id)kSecUseKeychain]=(__bridge id)chain;
 CFErrorRef error=NULL;SecKeyRef key=SecKeyCreateRandomKey((__bridge CFDictionaryRef)actual,&error);
 NSCAssert(key!=NULL,@"legacy key creation succeeds");
 NSDictionary *observed=CFBridgingRelease(SecKeyCopyAttributes(key));
 NSCAssert([observed[(__bridge id)kSecAttrIsExtractable] isEqual:@NO],@"actual key cannot be extracted");
 CFDataRef exported=SecKeyCopyExternalRepresentation(key,&error);
 NSCAssert(exported==NULL,@"Security blocks private export");
 if(error) { CFRelease(error);error=NULL; }
 NSData *challenge=[@"synthetic challenge" dataUsingEncoding:NSUTF8StringEncoding];
 CFDataRef signature=SecKeyCreateSignature(key,kSecKeyAlgorithmECDSASignatureMessageX962SHA256,(__bridge CFDataRef)challenge,&error);
 SecKeyRef publicKey=SecKeyCopyPublicKey(key);
 NSCAssert(signature && SecKeyVerifySignature(publicKey,kSecKeyAlgorithmECDSASignatureMessageX962SHA256,(__bridge CFDataRef)challenge,signature,&error),@"sign and verify");
 CFRelease(publicKey);CFRelease(signature);CFRelease(key);
 NSCAssert(SecKeychainDelete(chain)==errSecSuccess,@"remove only synthetic keychain");CFRelease(chain);
 puts("remote key policy and real keychain contracts passed");
 } return 0; }
