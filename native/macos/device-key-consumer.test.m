/**
 * [INPUT]: 依赖 consumer 纯接口与测试专用 classic Keychain。
 * [OUTPUT]: 验证精确标签、SPKI、签名闭域、错误保留与无副作用查询。
 * [POS]: 隔离 Security 回归；不调用短命 helper、官方 App、TCC 或用户 Keychain。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <stdio.h>
#import <string.h>
#import "device-key-consumer.h"

static SecKeychainRef isolatedKeychain;
static NSUInteger itemQueryCount;
static OSStatus forcedItemStatus = errSecSuccess;
static OSStatus lastItemStatus = errSecSuccess;
static NSString *expectedKeyId;

// --- 测试查询只能命中本测试创建的私有 Keychain。 ---
OSStatus incodexDeviceKeyTestItemCopyMatching(CFDictionaryRef query, CFTypeRef *result) {
 itemQueryCount++;
 NSDictionary *attributes=(__bridge NSDictionary *)query;
 NSArray *searchList=attributes[(__bridge id)kSecMatchSearchList];
 NSString *expectedTag=[@"com.openai.codex.device-key.os-protected-nonextractable." stringByAppendingString:expectedKeyId ?: @""];
 if(![searchList isKindOfClass:NSArray.class] || searchList.count!=1 ||
    !CFEqual((__bridge CFTypeRef)searchList.firstObject,isolatedKeychain) ||
    ![attributes[(__bridge id)kSecUseDataProtectionKeychain] isEqual:@NO] ||
    ![attributes[(__bridge id)kSecAttrApplicationTag] isEqual:[expectedTag dataUsingEncoding:NSUTF8StringEncoding]] ||
    ![attributes[(__bridge id)kSecAttrKeyClass] isEqual:(__bridge id)kSecAttrKeyClassPrivate] ||
    ![attributes[(__bridge id)kSecAttrKeyType] isEqual:(__bridge id)kSecAttrKeyTypeECSECPrimeRandom] ||
    ![attributes[(__bridge id)kSecAttrKeySizeInBits] isEqual:@256] ||
    ![attributes[(__bridge id)kSecAttrIsExtractable] isEqual:@NO] ||
    attributes[(__bridge id)kSecAttrTokenID]!=nil) return errSecParam;
 if(forcedItemStatus!=errSecSuccess) return forcedItemStatus;
 lastItemStatus=SecItemCopyMatching(query,result);
 return lastItemStatus;
}

static NSString *validKeyId(unichar character) {
 NSString *prefix=[@"" stringByPaddingToLength:42
   withString:[NSString stringWithCharacters:&character length:1] startingAtIndex:0];
 return [NSString stringWithFormat:@"dk_osn_%@A",prefix];
}

static NSString *keyTagName(NSString *keyId) {
 return [@"com.openai.codex.device-key.os-protected-nonextractable." stringByAppendingString:keyId];
}

static NSDictionary *publicRequest(NSString *keyId) {
 return @{@"schemaVersion":@1,@"operation":@"public",@"keyId":keyId};
}

static NSData *payloadWithType(NSString *type) {
 NSDictionary *payload=@{@"domain":@"codex-device-key-sign-payload/v1",
                         @"payload":@{@"type":type,@"challenge":@"synthetic fixture"}};
 return [NSJSONSerialization dataWithJSONObject:payload options:NSJSONWritingSortedKeys error:NULL];
}

static NSData *validPayload(void) { return payloadWithType(@"remoteControlClientEnrollment"); }

static NSDictionary *signRequest(NSString *keyId, NSData *payload) {
 return @{@"schemaVersion":@1,@"operation":@"sign",@"keyId":keyId,
          @"payloadBase64":[payload base64EncodedStringWithOptions:0]};
}

static SecKeyRef createFixtureKey(NSString *tag, SecKeychainRef keychain) {
 CFErrorRef error=NULL;
 SecAccessControlRef access=SecAccessControlCreateWithFlags(NULL,
   kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,kSecAccessControlPrivateKeyUsage,&error);
 NSCAssert(access!=NULL,@"create synthetic ACL");
 NSDictionary *parameters=@{(__bridge id)kSecAttrLabel:@"Isolated device-key fixture",
  (__bridge id)kSecAttrKeyType:(__bridge id)kSecAttrKeyTypeECSECPrimeRandom,
  (__bridge id)kSecAttrKeySizeInBits:@256,
  (__bridge id)kSecAttrIsExtractable:@NO,
  (__bridge id)kSecUseDataProtectionKeychain:@NO,
  (__bridge id)kSecUseKeychain:(__bridge id)keychain,
  (__bridge id)kSecPrivateKeyAttrs:@{
   (__bridge id)kSecAttrApplicationTag:[tag dataUsingEncoding:NSUTF8StringEncoding],
   (__bridge id)kSecAttrIsPermanent:@YES,
   (__bridge id)kSecAttrIsExtractable:@NO,
   (__bridge id)kSecAttrAccessControl:(__bridge id)access}};
 SecKeyRef key=SecKeyCreateRandomKey((__bridge CFDictionaryRef)parameters,&error);
 if(error) CFRelease(error);
 CFRelease(access);
 NSCAssert(key!=NULL,@"create non-extractable P-256 fixture");
 return key;
}

static NSArray<NSString *> *keychainFingerprint(SecKeychainRef keychain) {
 NSDictionary *query=@{(__bridge id)kSecClass:(__bridge id)kSecClassKey,
  (__bridge id)kSecMatchSearchList:@[(__bridge id)keychain],
  (__bridge id)kSecMatchLimit:(__bridge id)kSecMatchLimitAll,
  (__bridge id)kSecReturnAttributes:@YES,
  (__bridge id)kSecUseDataProtectionKeychain:@NO};
 CFTypeRef result=NULL;
 OSStatus status=SecItemCopyMatching((__bridge CFDictionaryRef)query,&result);
 NSCAssert(status==errSecSuccess,@"enumerate only isolated fixture Keychain");
 NSArray *attributes=CFBridgingRelease(result);
 NSMutableArray<NSString *> *items=[NSMutableArray array];
 for(NSDictionary *entry in attributes) {
  NSData *tag=entry[(__bridge id)kSecAttrApplicationTag];
  NSString *tag64=[tag base64EncodedStringWithOptions:0] ?: @"";
  [items addObject:[NSString stringWithFormat:@"%@:%@:%@:%@",tag64,
    entry[(__bridge id)kSecAttrKeyType] ?: @"",entry[(__bridge id)kSecAttrKeySizeInBits] ?: @"",
    entry[(__bridge id)kSecAttrIsExtractable] ?: @""]];
 }
 return [items sortedArrayUsingSelector:@selector(compare:)];
}

static NSDictionary *consume(NSDictionary *request, IncodexDeviceKeyConsumerOutcome *outcome,
                             OSStatus *securityStatus) {
 expectedKeyId=request[@"keyId"];
 return incodexDeviceKeyConsumeRequest(request,isolatedKeychain,outcome,securityStatus);
}

static void assertProtocolWithoutLookup(NSDictionary *request, NSString *name) {
 NSUInteger before=itemQueryCount;
 IncodexDeviceKeyConsumerOutcome outcome=IncodexDeviceKeyConsumerOutcomeSuccess;
 OSStatus status=-1;
 NSDictionary *result=consume(request,&outcome,&status);
 NSCAssert(result==nil && outcome==IncodexDeviceKeyConsumerOutcomeProtocolError &&
           status==errSecSuccess && itemQueryCount==before,@"%@ rejects before Keychain lookup",name);
}

int main(void) { @autoreleasepool {
 NSString *sandbox=[NSTemporaryDirectory() stringByAppendingPathComponent:
   [@"incodex-device-key-consumer-" stringByAppendingString:NSUUID.UUID.UUIDString]];
 NSError *directoryError=nil;
 BOOL directoryCreated=[[NSFileManager defaultManager] createDirectoryAtPath:sandbox
   withIntermediateDirectories:NO attributes:@{NSFilePosixPermissions:@0700} error:&directoryError];
 NSString *chainPath=[sandbox stringByAppendingPathComponent:@"fixture.keychain-db"];
 const char *password="fixture-only";
 SecKeyRef privateKey=NULL;
 SecKeyRef wrongKey=NULL;
 SecKeyRef publicKey=NULL;
 int exitStatus=0;
 @try {
  NSCAssert(directoryCreated && directoryError==nil,@"create private temporary test directory");
  OSStatus chainStatus=SecKeychainCreate(chainPath.fileSystemRepresentation,(UInt32)strlen(password),
    password,false,NULL,&isolatedKeychain);
  NSCAssert(chainStatus==errSecSuccess && isolatedKeychain!=NULL,@"create prompt-free synthetic Keychain");
  NSString *keyId=validKeyId('A');
  NSString *wrongTag=[@"com.openai.codex.device-key.os-protected-nonextractable." stringByAppendingString:validKeyId('B')];
  privateKey=createFixtureKey(keyTagName(keyId),isolatedKeychain);
  wrongKey=createFixtureKey(wrongTag,isolatedKeychain);
  NSDictionary *privateAttributes=CFBridgingRelease(SecKeyCopyAttributes(privateKey));
  NSCAssert([privateAttributes[(__bridge id)kSecAttrIsExtractable] isEqual:@NO],@"fixture stays non-extractable");
  CFErrorRef exportError=NULL;
  CFDataRef privateExport=SecKeyCopyExternalRepresentation(privateKey,&exportError);
  NSCAssert(privateExport==NULL,@"Security blocks private-key export");
  if(privateExport) CFRelease(privateExport);
  if(exportError) CFRelease(exportError);

  NSArray *before=keychainFingerprint(isolatedKeychain);
  IncodexDeviceKeyConsumerOutcome outcome=IncodexDeviceKeyConsumerOutcomeProtocolError;
  OSStatus securityStatus=-1;
  NSDictionary *publicResult=consume(publicRequest(keyId),&outcome,&securityStatus);
  NSCAssert(outcome==IncodexDeviceKeyConsumerOutcomeSuccess && securityStatus==errSecSuccess,
            @"exact public-key query succeeds (outcome=%ld status=%d query=%d)",
            (long)outcome,(int)securityStatus,(int)lastItemStatus);
  NSCAssert([publicResult[@"keyId"] isEqual:keyId] &&
            [publicResult[@"algorithm"] isEqual:@"ecdsa_p256_sha256"] &&
            [publicResult[@"protectionClass"] isEqual:@"os_protected_nonextractable"],
            @"public result preserves official response fields");
  NSData *returnedSPKI=[[NSData alloc] initWithBase64EncodedString:publicResult[@"publicKeySpkiDerBase64"] options:0];
  publicKey=SecKeyCopyPublicKey(privateKey);
  CFErrorRef publicError=NULL;
  CFDataRef publicBytes=SecKeyCopyExternalRepresentation(publicKey,&publicError);
  static const uint8_t spkiPrefix[]={0x30,0x59,0x30,0x13,0x06,0x07,0x2a,0x86,0x48,0xce,0x3d,0x02,0x01,
                                     0x06,0x08,0x2a,0x86,0x48,0xce,0x3d,0x03,0x01,0x07,0x03,0x42,0x00};
  NSMutableData *expectedSPKI=[NSMutableData dataWithBytes:spkiPrefix length:sizeof(spkiPrefix)];
  [expectedSPKI appendData:CFBridgingRelease(publicBytes)];
  NSCAssert([returnedSPKI isEqual:expectedSPKI],@"public result is the exact P-256 SPKI DER");
  if(publicError) CFRelease(publicError);

  NSData *payload=validPayload();
  NSDictionary *signResult=consume(signRequest(keyId,payload),&outcome,&securityStatus);
  NSCAssert(outcome==IncodexDeviceKeyConsumerOutcomeSuccess && securityStatus==errSecSuccess,
            @"allowlisted enrollment payload signs");
  NSCAssert([signResult[@"algorithm"] isEqual:@"ecdsa_p256_sha256"],@"signature algorithm is stable");
  NSData *signature=[[NSData alloc] initWithBase64EncodedString:signResult[@"signatureDerBase64"] options:0];
  CFErrorRef verifyError=NULL;
  NSCAssert(signature && SecKeyVerifySignature(publicKey,kSecKeyAlgorithmECDSASignatureMessageX962SHA256,
    (__bridge CFDataRef)payload,(__bridge CFDataRef)signature,&verifyError),@"returned DER signature verifies");
  if(verifyError) CFRelease(verifyError);
  NSData *connectionPayload=payloadWithType(@"remoteControlClientConnection");
  NSDictionary *connectionSign=consume(signRequest(keyId,connectionPayload),&outcome,&securityStatus);
  NSData *connectionSignature=[[NSData alloc] initWithBase64EncodedString:connectionSign[@"signatureDerBase64"] options:0];
  NSCAssert(outcome==IncodexDeviceKeyConsumerOutcomeSuccess && connectionSignature &&
    SecKeyVerifySignature(publicKey,kSecKeyAlgorithmECDSASignatureMessageX962SHA256,
      (__bridge CFDataRef)connectionPayload,(__bridge CFDataRef)connectionSignature,NULL),
    @"connection payload type also signs");

  NSDictionary *validWrongId=publicRequest(validKeyId('C'));
  NSDictionary *missingResult=consume(validWrongId,&outcome,&securityStatus);
  NSCAssert(missingResult==nil && outcome==IncodexDeviceKeyConsumerOutcomeSecurityError &&
            securityStatus==errSecItemNotFound,@"wrong-tag key remains item-missing");

  NSData *invalidJson=[@"[]" dataUsingEncoding:NSUTF8StringEncoding];
  NSData *wrongDomain=[NSJSONSerialization dataWithJSONObject:@{@"domain":@"other/v1",
    @"payload":@{@"type":@"remoteControlClientEnrollment"}} options:0 error:NULL];
  NSData *unknownType=[NSJSONSerialization dataWithJSONObject:@{@"domain":@"codex-device-key-sign-payload/v1",
    @"payload":@{@"type":@"hardware"}} options:0 error:NULL];
  NSDictionary *hardwareKeyId=@{@"schemaVersion":@1,@"operation":@"public",
    @"keyId":@"dk_hardware_AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"};
  NSDictionary *extraHardware=@{@"schemaVersion":@1,@"operation":@"public",@"keyId":keyId,
    @"hardware":@YES};
  NSDictionary *unknownOperation=@{@"schemaVersion":@1,@"operation":@"unknown",@"keyId":keyId};
  NSDictionary *invalidVersion=@{@"schemaVersion":@YES,@"operation":@"public",@"keyId":keyId};
  NSString *nonCanonicalId=[keyId stringByReplacingCharactersInRange:NSMakeRange(49,1) withString:@"B"];
  NSDictionary *invalidId=@{@"schemaVersion":@1,@"operation":@"public",@"keyId":nonCanonicalId};
  NSDictionary *invalidBase64=@{@"schemaVersion":@1,@"operation":@"sign",@"keyId":keyId,
    @"payloadBase64":@"%%%"};
  assertProtocolWithoutLookup(hardwareKeyId,@"hardware key id");
  assertProtocolWithoutLookup(extraHardware,@"hardware extension");
  assertProtocolWithoutLookup(unknownOperation,@"unknown operation");
  assertProtocolWithoutLookup(invalidVersion,@"boolean schema version");
  assertProtocolWithoutLookup(invalidId,@"non-canonical base64url key id");
  assertProtocolWithoutLookup(invalidBase64,@"invalid payload base64");
  assertProtocolWithoutLookup(signRequest(keyId,invalidJson),@"non-object payload");
  assertProtocolWithoutLookup(signRequest(keyId,wrongDomain),@"foreign payload domain");
  assertProtocolWithoutLookup(signRequest(keyId,unknownType),@"unknown payload type");
  NSData *missingType=[NSJSONSerialization dataWithJSONObject:@{@"domain":@"codex-device-key-sign-payload/v1",
    @"payload":@{@"challenge":@"missing type"}}
    options:0 error:NULL];
  assertProtocolWithoutLookup(signRequest(keyId,missingType),@"missing payload type");
  NSUInteger callsBeforeDenied=itemQueryCount;
  forcedItemStatus=errSecInteractionNotAllowed;
  NSDictionary *denied=consume(publicRequest(keyId),&outcome,&securityStatus);
  forcedItemStatus=errSecSuccess;
  NSCAssert(denied==nil && outcome==IncodexDeviceKeyConsumerOutcomeSecurityError &&
            securityStatus==errSecInteractionNotAllowed && itemQueryCount==callsBeforeDenied+1,
            @"Security denial returns its exact OSStatus");
  NSCAssert([keychainFingerprint(isolatedKeychain) isEqual:before],
            @"consumer leaves the isolated keychain search fingerprint unchanged");

  puts("device-key consumer isolated Keychain contracts passed");
 } @catch(NSException *exception) {
  fprintf(stderr,"device-key consumer assertion failed: %s\n",exception.reason.UTF8String ?: "unknown");
  exitStatus=1;
 } @finally {
  if(publicKey) CFRelease(publicKey);
  if(wrongKey) CFRelease(wrongKey);
  if(privateKey) CFRelease(privateKey);
  if(isolatedKeychain) {
   if(SecKeychainDelete(isolatedKeychain)!=errSecSuccess) exitStatus=1;
   CFRelease(isolatedKeychain);
  }
  [[NSFileManager defaultManager] removeItemAtPath:sandbox error:NULL];
 }
 return exitStatus;
 } }
