/**
 * [INPUT]: 依赖真实 Security 签名 API 与测试传入的私有临时目录。
 * [OUTPUT]: 输出合成宿主身份；只读诊断隔离身份的查询差异。
 * [POS]: 宿主身份实签回归；只签临时可执行文件，不进入官方包。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <CommonCrypto/CommonDigest.h>
#import <dlfcn.h>

static NSString *ownedHome;
static NSString *fixtureHome(void) { return ownedHome; }
#define NSHomeDirectory fixtureHome
#import "host-identity.h"
#undef NSHomeDirectory

// --- 诊断专用：镜像 Apple codesign 的身份枚举，不签名、不改偏好。 ---
typedef OSStatus (*IdentitySearchQuery)(SecPolicyRef, CFStringRef, uint32, CFTypeRef, Boolean, SecIdentitySearchRef *);

static void queryScope(const char *label, IdentitySearchQuery query, SecPolicyRef policy,
                       CFTypeRef scope, uint32 usage, const char *expected) {
 SecIdentitySearchRef search=NULL;
 OSStatus created=query(policy,NULL,usage,scope,false,&search), ended=created;
 unsigned count=0; bool matched=false;
 if(created==errSecSuccess && search) {
  // 仅枚举本测试的一份身份；超出八项保留截断，不扫描用户库。
  for(unsigned i=0;i<8;i++) {
   SecIdentityRef identity=NULL;
   ended=SecIdentitySearchCopyNext(search,&identity);
   if(ended!=errSecSuccess || !identity) break;
   count++;
   SecCertificateRef certificate=NULL;
   if(SecIdentityCopyCertificate(identity,&certificate)==errSecSuccess && certificate) {
    CFDataRef data=SecCertificateCopyData(certificate);
    if(data) {
     unsigned char hash[CC_SHA1_DIGEST_LENGTH]; char hex[41];
     CC_SHA1(CFDataGetBytePtr(data),(CC_LONG)CFDataGetLength(data),hash);
     for(unsigned n=0;n<CC_SHA1_DIGEST_LENGTH;n++) snprintf(hex+n*2,3,"%02x",hash[n]);
     hex[40]=0; matched=matched || strcasecmp(hex,expected)==0; CFRelease(data);
    }
    CFRelease(certificate);
   }
   CFRelease(identity);
  }
  CFRelease(search);
 }
 printf("%s created=%d ended=%d count=%u matched=%d\n",label,(int)created,(int)ended,count,matched);
}

// --- 按本机codesign实际导入的SecItemCopyMatching构造同形查询。 ---
static void queryModern(const char *label, SecKeychainRef keychain, SecPolicyRef policy,
                        bool requireSign, const char *expected) {
 NSMutableDictionary *attributes=[@{(__bridge id)kSecClass:(__bridge id)kSecClassIdentity,
  (__bridge id)kSecMatchSearchList:@[(__bridge id)keychain],
  (__bridge id)kSecMatchLimit:(__bridge id)kSecMatchLimitAll,
  (__bridge id)kSecReturnRef:@YES} mutableCopy];
 if(policy) attributes[(__bridge id)kSecMatchPolicy]=(__bridge id)policy;
 if(requireSign) attributes[(__bridge id)kSecAttrCanSign]=@YES;
 CFTypeRef result=NULL;
 OSStatus status=SecItemCopyMatching((__bridge CFDictionaryRef)attributes,&result);
 unsigned count=0; bool matched=false;
 if(status==errSecSuccess && result && CFGetTypeID(result)==CFArrayGetTypeID()) {
  CFArrayRef array=(CFArrayRef)result;
  count=(unsigned)CFArrayGetCount(array);
  for(CFIndex i=0;i<CFArrayGetCount(array) && i<8;i++) {
   SecIdentityRef identity=(SecIdentityRef)CFArrayGetValueAtIndex(array,i);
   SecCertificateRef certificate=NULL;
   if(SecIdentityCopyCertificate(identity,&certificate)==errSecSuccess && certificate) {
    CFDataRef data=SecCertificateCopyData(certificate);
    if(data) {
     unsigned char hash[CC_SHA1_DIGEST_LENGTH]; char hex[41];
     CC_SHA1(CFDataGetBytePtr(data),(CC_LONG)CFDataGetLength(data),hash);
     for(unsigned n=0;n<CC_SHA1_DIGEST_LENGTH;n++) snprintf(hex+n*2,3,"%02x",hash[n]);
     hex[40]=0; matched=matched || strcasecmp(hex,expected)==0; CFRelease(data);
    }
    CFRelease(certificate);
   }
  }
 }
 printf("%s status=%d count=%u matched=%d\n",label,(int)status,count,matched);
 if(result) CFRelease(result);
}

static int queryIdentity(const char *path, const char *expected) {
 (void)SecKeychainSetUserInteractionAllowed(false);
 SecKeychainRef keychain=NULL;
 OSStatus opened=SecKeychainOpen(path,&keychain);
 printf("private-keychain-open=%d\n",(int)opened);
 if(opened!=errSecSuccess || !keychain) return 0;
 // SPI只存在于未发布的诊断夹具，不进入生产或Runtime。
 IdentitySearchQuery query=(IdentitySearchQuery)dlsym(RTLD_DEFAULT,"SecIdentitySearchCreateWithPolicy");
 SecPolicyRef policy=SecPolicyCreateWithProperties(kSecPolicyAppleCodeSigning,NULL);
 if(query && policy) {
  const void *value=keychain;
  CFArrayRef array=CFArrayCreate(NULL,&value,1,&kCFTypeArrayCallBacks);
  queryScope("direct-sign",query,policy,keychain,CSSM_KEYUSE_SIGN,expected);
  queryScope("array-sign",query,policy,array,CSSM_KEYUSE_SIGN,expected);
  queryScope("direct-any",query,policy,keychain,0,expected);
  queryScope("array-any",query,policy,array,0,expected);
  CFRelease(array);
 } else printf("query-available=%d policy-available=%d\n",query!=NULL,policy!=NULL);
 queryModern("modern-sign-policy",keychain,policy,true,expected);
 queryModern("modern-policy-only",keychain,policy,false,expected);
 queryModern("modern-sign-only",keychain,NULL,true,expected);
 queryModern("modern-bare",keychain,NULL,false,expected);
 CFStringRef selector=CFStringCreateWithCString(NULL,expected,kCFStringEncodingASCII);
 SecIdentityRef preference=NULL;
 OSStatus preferred=SecIdentityCopyPreference(selector,0,NULL,&preference);
 printf("preference-status=%d has-identity=%d\n",(int)preferred,preference!=NULL);
 if(preference) CFRelease(preference);
 CFRelease(selector); if(policy) CFRelease(policy); CFRelease(keychain);
 return 0;
}

int main(int argc, const char *argv[]) { @autoreleasepool {
 if(argc==4 && strcmp(argv[1],"--identity-query")==0) return queryIdentity(argv[2],argv[3]);
 if(argc!=2) return 64;
 ownedHome=[NSString stringWithUTF8String:argv[1]];
 switch(incodexHostIdentity()) {
  case IncodexHostIdentityRegisteredLocal: puts("registered-local"); break;
  case IncodexHostIdentityAdhoc: puts("ad-hoc"); break;
  default: puts("unsupported"); break;
 }
 return 0;
} }
