/**
 * [INPUT]: 依赖闭域 device-key 请求、classic Keychain 与只读宿主身份登记。
 * [OUTPUT]: 提供官方 P-256 公钥或受限签名，并报告闭合 OSStatus。
 * [POS]: 短命 macOS device-key helper；验证精确宿主，不改写既有 Storage helper。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import "device-key-consumer.h"

#if !defined(INCODEX_DEVICE_KEY_CONSUMER_TESTING)
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wunused-function"
#import "host-identity.h"
#pragma clang diagnostic pop
#import <errno.h>
#import <dispatch/dispatch.h>
#import <libproc.h>
#import <signal.h>
#import <string.h>
#import <sys/proc_info.h>
#import <unistd.h>
#endif

static NSString * const incodexDeviceKeyPrefix = @"com.openai.codex.device-key.os-protected-nonextractable.";
static NSString * const incodexDeviceKeyAlgorithm = @"ecdsa_p256_sha256";
static const size_t incodexDeviceKeyMaxInputBytes = 64 * 1024;
static const size_t incodexDeviceKeyMaxPayloadBytes = 48 * 1024;

static OSStatus incodexDeviceKeyCopyItem(CFDictionaryRef query, CFTypeRef *result) {
#if defined(INCODEX_DEVICE_KEY_CONSUMER_TESTING)
 return incodexDeviceKeyTestItemCopyMatching(query,result);
#else
 return SecItemCopyMatching(query,result);
#endif
}

static void incodexDeviceKeySetFailure(IncodexDeviceKeyConsumerOutcome kind, OSStatus status,
                                       IncodexDeviceKeyConsumerOutcome *outcome,
                                       OSStatus *securityStatus) {
 if(outcome) *outcome=kind;
 if(securityStatus) *securityStatus=status;
}

static bool incodexDeviceKeyValidId(NSString *keyId) {
 if(![keyId isKindOfClass:NSString.class] || keyId.length!=50 ||
    ![keyId hasPrefix:@"dk_osn_"]) return false;
 for(NSUInteger index=7;index<keyId.length;index++) {
  unichar character=[keyId characterAtIndex:index];
  if(!((character>='A' && character<='Z') || (character>='a' && character<='z') ||
       (character>='0' && character<='9') || character=='_' || character=='-')) return false;
 }
 NSString *suffix=[keyId substringFromIndex:7];
 NSString *standard=[[[suffix stringByReplacingOccurrencesOfString:@"-" withString:@"+"]
   stringByReplacingOccurrencesOfString:@"_" withString:@"/"] stringByAppendingString:@"="];
 NSData *decoded=[[NSData alloc] initWithBase64EncodedString:standard options:0];
 NSString *canonical=[[decoded base64EncodedStringWithOptions:0] stringByReplacingOccurrencesOfString:@"=" withString:@""];
 canonical=[[canonical stringByReplacingOccurrencesOfString:@"+" withString:@"-"]
   stringByReplacingOccurrencesOfString:@"/" withString:@"_"];
 return decoded.length==32 && [canonical isEqual:suffix];
}

static bool incodexDeviceKeyValidSchemaVersion(id value) {
 if(![value isKindOfClass:NSNumber.class] ||
    CFGetTypeID((__bridge CFTypeRef)value)==CFBooleanGetTypeID()) return false;
 return !CFNumberIsFloatType((__bridge CFNumberRef)value) && [value isEqual:@1];
}

static bool incodexDeviceKeyValidBase64(NSString *encoded, NSData **decoded) {
 if(![encoded isKindOfClass:NSString.class] || encoded.length==0 ||
    encoded.length>incodexDeviceKeyMaxInputBytes || encoded.length%4!=0) return false;
 NSUInteger padding=0;
 for(NSUInteger index=0;index<encoded.length;index++) {
  unichar character=[encoded characterAtIndex:index];
  bool alphabet=(character>='A' && character<='Z') || (character>='a' && character<='z') ||
                (character>='0' && character<='9') || character=='+' || character=='/';
  if(alphabet && padding==0) continue;
  if(character=='=' && index>=encoded.length-2) { padding++; continue; }
  return false;
 }
 if(padding>2) return false;
 NSData *bytes=[[NSData alloc] initWithBase64EncodedString:encoded options:0];
 if(!bytes || bytes.length==0 || bytes.length>incodexDeviceKeyMaxPayloadBytes ||
    ![[bytes base64EncodedStringWithOptions:0] isEqual:encoded]) return false;
 if(decoded) *decoded=bytes;
 return true;
}

static bool incodexDeviceKeyValidPayload(NSData *bytes) {
 id value=[NSJSONSerialization JSONObjectWithData:bytes options:0 error:NULL];
 if(![value isKindOfClass:NSDictionary.class]) return false;
 NSDictionary *payload=(NSDictionary *)value;
 NSDictionary *inner=payload[@"payload"];
 if(![payload[@"domain"] isEqual:@"codex-device-key-sign-payload/v1"] ||
    ![inner isKindOfClass:NSDictionary.class]) return false;
 id type=inner[@"type"];
 return [type isKindOfClass:NSString.class] &&
        ([type isEqual:@"remoteControlClientEnrollment"] ||
         [type isEqual:@"remoteControlClientConnection"]);
}

static bool incodexDeviceKeyValidRequest(NSDictionary *request, NSString **keyId,
                                         NSData **payloadBytes, NSString **operation) {
 if(![request isKindOfClass:NSDictionary.class] ||
    !incodexDeviceKeyValidSchemaVersion(request[@"schemaVersion"])) return false;
 id operationValue=request[@"operation"];
 if(![operationValue isKindOfClass:NSString.class] ||
    (![operationValue isEqual:@"public"] && ![operationValue isEqual:@"sign"])) return false;
 BOOL signing=[operationValue isEqual:@"sign"];
 NSSet *allowed=signing ? [NSSet setWithArray:@[@"schemaVersion",@"operation",@"keyId",@"payloadBase64"]] :
                          [NSSet setWithArray:@[@"schemaVersion",@"operation",@"keyId"]];
 if(request.count!=allowed.count) return false;
 for(id name in request) if(![name isKindOfClass:NSString.class] || ![allowed containsObject:name]) return false;
 id identifier=request[@"keyId"];
 if(![identifier isKindOfClass:NSString.class] || !incodexDeviceKeyValidId(identifier)) return false;
 NSData *bytes=nil;
 if(signing && (!incodexDeviceKeyValidBase64(request[@"payloadBase64"],&bytes) ||
                !incodexDeviceKeyValidPayload(bytes))) return false;
 if(keyId) *keyId=identifier;
 if(payloadBytes) *payloadBytes=bytes;
 if(operation) *operation=operationValue;
 return true;
}

static OSStatus incodexDeviceKeyErrorStatus(CFErrorRef error) {
 if(error && CFEqual(CFErrorGetDomain(error),kCFErrorDomainOSStatus))
  return (OSStatus)CFErrorGetCode(error);
 return errSecInternalComponent;
}

static bool incodexDeviceKeyExactPrivateKey(SecKeyRef key) {
 CFDictionaryRef copied=SecKeyCopyAttributes(key);
 if(!copied) return false;
 NSDictionary *attributes=CFBridgingRelease(copied);
 return [attributes[(__bridge id)kSecAttrKeyType] isEqual:(__bridge id)kSecAttrKeyTypeECSECPrimeRandom] &&
        [attributes[(__bridge id)kSecAttrKeyClass] isEqual:(__bridge id)kSecAttrKeyClassPrivate] &&
        [attributes[(__bridge id)kSecAttrKeySizeInBits] isEqual:@256] &&
        [attributes[(__bridge id)kSecAttrIsExtractable] isEqual:@NO] &&
        attributes[(__bridge id)kSecAttrTokenID]==nil;
}

static SecKeyRef incodexDeviceKeyFindPrivateKey(NSData *tag, SecKeychainRef keychain,
                                                OSStatus *status) {
 NSMutableDictionary *query=[@{(__bridge id)kSecClass:(__bridge id)kSecClassKey,
  (__bridge id)kSecAttrApplicationTag:tag,
  (__bridge id)kSecAttrKeyType:(__bridge id)kSecAttrKeyTypeECSECPrimeRandom,
  (__bridge id)kSecAttrKeyClass:(__bridge id)kSecAttrKeyClassPrivate,
  (__bridge id)kSecAttrKeySizeInBits:@256,
  (__bridge id)kSecAttrIsExtractable:@NO,
  (__bridge id)kSecUseDataProtectionKeychain:@NO,
  (__bridge id)kSecReturnRef:@YES,
  (__bridge id)kSecMatchLimit:(__bridge id)kSecMatchLimitOne} mutableCopy];
 if(keychain) query[(__bridge id)kSecMatchSearchList]=@[(__bridge id)keychain];
 CFTypeRef result=NULL;
 OSStatus found=incodexDeviceKeyCopyItem((__bridge CFDictionaryRef)query,&result);
 if(status) *status=found;
 if(found!=errSecSuccess) { if(result) CFRelease(result);return NULL; }
 if(!result || CFGetTypeID(result)!=SecKeyGetTypeID()) {
  if(result) CFRelease(result);
  if(status) *status=errSecNotAvailable;
  return NULL;
 }
 SecKeyRef key=(SecKeyRef)result;
 if(!incodexDeviceKeyExactPrivateKey(key)) {
  CFRelease(key);
  if(status) *status=errSecNotAvailable;
  return NULL;
 }
 return key;
}

static NSString *incodexDeviceKeyPublicSPKI(SecKeyRef privateKey, OSStatus *status) {
 CFErrorRef error=NULL;
 SecKeyRef publicKey=SecKeyCopyPublicKey(privateKey);
 if(!publicKey) {
  if(status) *status=errSecInternalComponent;
  return nil;
 }
 CFDataRef raw=SecKeyCopyExternalRepresentation(publicKey,&error);
 CFRelease(publicKey);
 if(!raw) {
  if(status) *status=incodexDeviceKeyErrorStatus(error);
  if(error) CFRelease(error);
  return nil;
 }
 if(error) CFRelease(error);
 static const uint8_t prefix[]={0x30,0x59,0x30,0x13,0x06,0x07,0x2a,0x86,0x48,0xce,0x3d,0x02,0x01,
                                0x06,0x08,0x2a,0x86,0x48,0xce,0x3d,0x03,0x01,0x07,0x03,0x42,0x00};
 if(CFDataGetLength(raw)!=65 || CFDataGetBytePtr(raw)[0]!=0x04) {
  CFRelease(raw);
  if(status) *status=errSecNotAvailable;
  return nil;
 }
 NSMutableData *spki=[NSMutableData dataWithBytes:prefix length:sizeof(prefix)];
 [spki appendBytes:CFDataGetBytePtr(raw) length:CFDataGetLength(raw)];
 CFRelease(raw);
 if(status) *status=errSecSuccess;
 return [spki base64EncodedStringWithOptions:0];
}

static NSString *incodexDeviceKeySign(SecKeyRef key, NSData *payload, OSStatus *status) {
 CFErrorRef error=NULL;
 CFDataRef signature=SecKeyCreateSignature(key,kSecKeyAlgorithmECDSASignatureMessageX962SHA256,
                                            (__bridge CFDataRef)payload,&error);
 if(!signature) {
  if(status) *status=incodexDeviceKeyErrorStatus(error);
  if(error) CFRelease(error);
  return nil;
 }
 if(error) CFRelease(error);
 NSString *encoded=[(__bridge NSData *)signature base64EncodedStringWithOptions:0];
 CFRelease(signature);
 if(status) *status=errSecSuccess;
 return encoded;
}

NSDictionary *incodexDeviceKeyConsumeRequest(NSDictionary *request, SecKeychainRef keychain,
                                              IncodexDeviceKeyConsumerOutcome *outcome,
                                              OSStatus *securityStatus) {
 incodexDeviceKeySetFailure(IncodexDeviceKeyConsumerOutcomeProtocolError,errSecSuccess,outcome,securityStatus);
 NSString *operation=nil,*keyId=nil;
 NSData *payload=nil;
 if(!incodexDeviceKeyValidRequest(request,&keyId,&payload,&operation)) return nil;
 NSString *tagName=[incodexDeviceKeyPrefix stringByAppendingString:keyId];
 NSData *tag=[tagName dataUsingEncoding:NSUTF8StringEncoding];
 OSStatus status=errSecSuccess;
 SecKeyRef privateKey=incodexDeviceKeyFindPrivateKey(tag,keychain,&status);
 if(!privateKey) {
  incodexDeviceKeySetFailure(IncodexDeviceKeyConsumerOutcomeSecurityError,status,outcome,securityStatus);
  return nil;
 }
 NSDictionary *result=nil;
 if([operation isEqual:@"public"]) {
  NSString *publicKey=incodexDeviceKeyPublicSPKI(privateKey,&status);
  if(publicKey) result=@{@"keyId":keyId,@"algorithm":incodexDeviceKeyAlgorithm,
                         @"protectionClass":@"os_protected_nonextractable",
                         @"publicKeySpkiDerBase64":publicKey};
 } else {
  NSString *signature=incodexDeviceKeySign(privateKey,payload,&status);
  if(signature) result=@{@"algorithm":incodexDeviceKeyAlgorithm,@"signatureDerBase64":signature};
 }
 CFRelease(privateKey);
 if(!result) {
  incodexDeviceKeySetFailure(IncodexDeviceKeyConsumerOutcomeSecurityError,status,outcome,securityStatus);
  return nil;
 }
 incodexDeviceKeySetFailure(IncodexDeviceKeyConsumerOutcomeSuccess,errSecSuccess,outcome,securityStatus);
 return result;
}

#if !defined(INCODEX_DEVICE_KEY_CONSUMER_TESTING)
static bool incodexDeviceKeyReadCaller(pid_t pid, struct proc_bsdinfo *information,
                                      char *path, size_t pathCapacity) {
 if(pid<=1 || proc_pidinfo(pid,PROC_PIDTBSDINFO,0,information,sizeof(*information))!=sizeof(*information) ||
    information->pbi_pid!=(uint32_t)pid || !path || pathCapacity<PROC_PIDPATHINFO_MAXSIZE) return false;
 return proc_pidpath(pid,path,(uint32_t)pathCapacity)>0;
}

static bool incodexDeviceKeySameCaller(const struct proc_bsdinfo *before,
                                      const struct proc_bsdinfo *after) {
 return before->pbi_pid==after->pbi_pid && before->pbi_uid==after->pbi_uid &&
        before->pbi_ruid==after->pbi_ruid && before->pbi_start_tvsec==after->pbi_start_tvsec &&
        before->pbi_start_tvusec==after->pbi_start_tvusec;
}

static dispatch_source_t incodexDeviceKeyWatchParentExit(pid_t parentPID,
  const struct proc_bsdinfo *expectedParent, const char *expectedPath) {
 if(parentPID<=1 || !expectedParent || !expectedPath || expectedPath[0]=='\0') return NULL;
 dispatch_source_t source=dispatch_source_create(DISPATCH_SOURCE_TYPE_PROC,(uintptr_t)parentPID,
   DISPATCH_PROC_EXIT,dispatch_get_global_queue(QOS_CLASS_USER_INITIATED,0));
 if(!source) return NULL;
 dispatch_source_set_event_handler(source,^{_exit(69);});
 dispatch_resume(source);
 struct proc_bsdinfo current={0};
 char currentPath[PROC_PIDPATHINFO_MAXSIZE]={0};
 if(getppid()!=parentPID ||
    !incodexDeviceKeyReadCaller(parentPID,&current,currentPath,sizeof(currentPath)) ||
    !incodexDeviceKeySameCaller(expectedParent,&current) || strcmp(currentPath,expectedPath)!=0) {
  dispatch_source_cancel(source);
  return NULL;
 }
 return source;
}

#if defined(INCODEX_DEVICE_KEY_LIFECYCLE_TESTING)
dispatch_source_t incodexDeviceKeyTestWatchParentExit(pid_t parentPID,
  const struct proc_bsdinfo *expectedParent, const char *expectedPath) {
 return incodexDeviceKeyWatchParentExit(parentPID,expectedParent,expectedPath);
}
#endif

#if !defined(INCODEX_DEVICE_KEY_LIFECYCLE_TESTING)
static const char *incodexDeviceKeyOfficialExecutable="/Applications/ChatGPT.app/Contents/MacOS/ChatGPT";
static NSString * const incodexDeviceKeyOfficialRequirement=
 @"anchor apple generic and certificate leaf[subject.OU] = \"2DC432GLL2\" and identifier \"com.openai.codex\"";

static NSData *incodexDeviceKeyLeafCertificate(NSDictionary *information) {
 id certificates=information[(__bridge id)kSecCodeInfoCertificates];
 if(![certificates isKindOfClass:NSArray.class] || [certificates count]==0) return nil;
 CFTypeRef leaf=(__bridge CFTypeRef)[certificates firstObject];
 if(CFGetTypeID(leaf)!=SecCertificateGetTypeID()) return nil;
 return CFBridgingRelease(SecCertificateCopyData((SecCertificateRef)leaf));
}

static bool incodexDeviceKeySignedByAllowedIdentity(SecCodeRef code, NSDictionary *information) {
 if(![information[(__bridge id)kSecCodeInfoIdentifier] isEqual:@"com.openai.codex"]) return false;
 uint32_t flags=[information[(__bridge id)kSecCodeInfoFlags] unsignedIntValue];
 if(flags&kSecCodeSignatureAdhoc) return false;
 NSData *leaf=incodexDeviceKeyLeafCertificate(information);
 NSData *registered=incodexRegisteredSigningCertificate();
 id team=information[(__bridge id)kSecCodeInfoTeamIdentifier];
 bool noTeam=![team isKindOfClass:NSString.class] || [team length]==0;
 if(registered && noTeam && [leaf isEqual:registered] &&
    SecCodeCheckValidity(code,kSecCSStrictValidate,NULL)==errSecSuccess) return true;
 SecRequirementRef requirement=NULL;
 OSStatus created=SecRequirementCreateWithString((__bridge CFStringRef)incodexDeviceKeyOfficialRequirement,
                                                   kSecCSDefaultFlags,&requirement);
 bool official=created==errSecSuccess && requirement &&
   SecCodeCheckValidity(code,kSecCSStrictValidate,requirement)==errSecSuccess;
 if(requirement) CFRelease(requirement);
 return official;
}

static bool incodexDeviceKeyTrustedCaller(struct proc_bsdinfo *verifiedIdentity) {
 pid_t caller=getppid();
 if(getuid()!=geteuid()) return false;
 struct proc_bsdinfo before={0},after={0};
 char path[PROC_PIDPATHINFO_MAXSIZE]={0},afterPath[PROC_PIDPATHINFO_MAXSIZE]={0};
 if(!incodexDeviceKeyReadCaller(caller,&before,path,sizeof(path)) ||
    strcmp(path,incodexDeviceKeyOfficialExecutable)!=0 || before.pbi_uid!=(uint32_t)geteuid() ||
    before.pbi_ruid!=(uint32_t)getuid()) return false;
 CFNumberRef pidNumber=CFNumberCreate(NULL,kCFNumberIntType,&caller);
 if(!pidNumber) return false;
 NSDictionary *attributes=@{(__bridge id)kSecGuestAttributePid:(__bridge id)pidNumber};
 CFRelease(pidNumber);
 SecCodeRef guest=NULL;
 if(SecCodeCopyGuestWithAttributes(NULL,(__bridge CFDictionaryRef)attributes,kSecCSDefaultFlags,&guest)!=errSecSuccess || !guest)
  return false;
 CFDictionaryRef rawInformation=NULL;
 bool trusted=false;
 if(SecCodeCopySigningInformation(guest,kSecCSSigningInformation,&rawInformation)==errSecSuccess && rawInformation) {
  NSDictionary *information=CFBridgingRelease(rawInformation);
  trusted=incodexDeviceKeySignedByAllowedIdentity(guest,information);
 }
 if(!incodexDeviceKeyReadCaller(caller,&after,afterPath,sizeof(afterPath)) ||
    !incodexDeviceKeySameCaller(&before,&after) || strcmp(afterPath,incodexDeviceKeyOfficialExecutable)!=0 ||
    getppid()!=caller) trusted=false;
 CFRelease(guest);
 if(trusted && verifiedIdentity) *verifiedIdentity=before;
 return trusted;
}

typedef NS_ENUM(NSInteger, IncodexDeviceKeyReadResult) {
 IncodexDeviceKeyReadResultSuccess,
 IncodexDeviceKeyReadResultProtocolError,
 IncodexDeviceKeyReadResultIOError,
};

static IncodexDeviceKeyReadResult incodexDeviceKeyReadRequest(NSData **requestBytes) {
 NSMutableData *bytes=[NSMutableData data];
 uint8_t buffer[4096];
 while(true) {
  ssize_t count=read(STDIN_FILENO,buffer,sizeof(buffer));
  if(count<0 && errno==EINTR) continue;
  if(count<0) return IncodexDeviceKeyReadResultIOError;
  if(count==0) break;
  if((size_t)count>incodexDeviceKeyMaxInputBytes-bytes.length)
   return IncodexDeviceKeyReadResultProtocolError;
  [bytes appendBytes:buffer length:(NSUInteger)count];
 }
 if(bytes.length==0) return IncodexDeviceKeyReadResultProtocolError;
 if(requestBytes) *requestBytes=bytes;
 return IncodexDeviceKeyReadResultSuccess;
}

static bool incodexDeviceKeyWriteAll(int descriptor, const uint8_t *bytes, size_t length) {
 size_t offset=0;
 while(offset<length) {
  ssize_t count=write(descriptor,bytes+offset,length-offset);
  if(count<0 && errno==EINTR) continue;
  if(count<=0) return false;
  offset+=(size_t)count;
 }
 return true;
}

static int incodexDeviceKeyWriteResponse(NSDictionary *response) {
 NSData *body=[NSJSONSerialization dataWithJSONObject:response options:0 error:NULL];
 if(!body) return 74;
 NSMutableData *frame=[body mutableCopy];
 const uint8_t newline='\n';
 [frame appendBytes:&newline length:1];
 return incodexDeviceKeyWriteAll(STDOUT_FILENO,frame.bytes,frame.length) ? 0 : 74;
}

int main(int argc, const char *argv[]) { @autoreleasepool {
 (void)argv;
 signal(SIGPIPE,SIG_IGN);
 if(argc!=1) return 64;
 struct proc_bsdinfo caller={0};
 if(!incodexDeviceKeyTrustedCaller(&caller)) return 69;
 __attribute__((objc_precise_lifetime)) dispatch_source_t parentExit=
   incodexDeviceKeyWatchParentExit(caller.pbi_pid,&caller,incodexDeviceKeyOfficialExecutable);
 if(!parentExit) return 69;
 NSData *requestBytes=nil;
 IncodexDeviceKeyReadResult readResult=incodexDeviceKeyReadRequest(&requestBytes);
 if(readResult==IncodexDeviceKeyReadResultIOError) return 74;
 if(readResult!=IncodexDeviceKeyReadResultSuccess) return 64;
 id request=[NSJSONSerialization JSONObjectWithData:requestBytes options:0 error:NULL];
 if(![request isKindOfClass:NSDictionary.class]) return 64;
 IncodexDeviceKeyConsumerOutcome outcome=IncodexDeviceKeyConsumerOutcomeProtocolError;
 OSStatus status=errSecSuccess;
 NSDictionary *result=incodexDeviceKeyConsumeRequest(request,NULL,&outcome,&status);
 if(outcome==IncodexDeviceKeyConsumerOutcomeProtocolError) return 64;
 if(outcome==IncodexDeviceKeyConsumerOutcomeSecurityError)
  return incodexDeviceKeyWriteResponse(@{@"ok":@NO,@"status":@(status)});
 if(!result) return 74;
 return incodexDeviceKeyWriteResponse(@{@"ok":@YES,@"result":result});
 } }
#endif
#endif
