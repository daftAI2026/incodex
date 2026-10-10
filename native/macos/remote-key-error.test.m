#import <Foundation/Foundation.h>
#import <Security/Security.h>
static SecKeyRef fixtureKey;
static int calls;
static SecKeyRef fakeCreate(CFDictionaryRef parameters,CFErrorRef *error);
#define SecKeyCreateRandomKey fakeCreate
#import "remote-key-compat.m"
#undef SecKeyCreateRandomKey
static SecKeyRef fakeCreate(CFDictionaryRef parameters,CFErrorRef *error) {
 if(calls++==0) { *error=CFErrorCreate(NULL,kCFErrorDomainOSStatus,errSecMissingEntitlement,NULL);return NULL; }
 // Core Foundation success APIs need not clear an error out-parameter.
 return (SecKeyRef)CFRetain(fixtureKey);
}
int main(void) { @autoreleasepool {
 NSString *location=[NSTemporaryDirectory() stringByAppendingPathComponent:[@"incodex-error-test-" stringByAppendingString:NSUUID.UUID.UUIDString]];
 SecKeychainRef chain=NULL;
 if(SecKeychainCreate(location.UTF8String,8,"testpass",false,NULL,&chain)!=0) return 2;
 @try {
  SecAccessControlRef acl=SecAccessControlCreateWithFlags(NULL,kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,kSecAccessControlPrivateKeyUsage,NULL);
  NSDictionary *request=@{(__bridge id)kSecAttrKeyType:(__bridge id)kSecAttrKeyTypeECSECPrimeRandom,
   (__bridge id)kSecAttrKeySizeInBits:@256,(__bridge id)kSecPrivateKeyAttrs:@{
    (__bridge id)kSecAttrApplicationTag:[@"com.openai.codex.device-key.error-fixture" dataUsingEncoding:NSUTF8StringEncoding],
    (__bridge id)kSecAttrIsPermanent:@YES,(__bridge id)kSecAttrIsExtractable:@NO,(__bridge id)kSecAttrAccessControl:(__bridge id)acl}};
  CFDictionaryRef retry=incodexRemoteKeyRetryAttributes((__bridge CFDictionaryRef)request);
  NSMutableDictionary *parameters=[(__bridge NSDictionary *)retry mutableCopy];parameters[(__bridge id)kSecUseKeychain]=(__bridge id)chain;
  fixtureKey=SecKeyCreateRandomKey((__bridge CFDictionaryRef)parameters,NULL);CFRelease(retry);CFRelease(acl);
  if(!fixtureKey) return 3;
  installedImage=_dyld_get_image_header(0);
  CFErrorRef error=NULL;SecKeyRef result=compatibleCreate((__bridge CFDictionaryRef)request,&error);
  bool passed=result && error==NULL && calls==2;
  if(result) CFRelease(result);CFRelease(fixtureKey);
  if(!passed) { puts("retry leaked the released original error");return 1; }
  puts("remote key retry error ownership passed");return 0;
 } @finally { SecKeychainDelete(chain);CFRelease(chain); }
} }
