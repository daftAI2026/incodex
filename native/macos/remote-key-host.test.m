/**
 * [INPUT]: 依赖旧 remote-key-compat 宿主门、合成 Security 签名与专有临时登记。
 * [OUTPUT]: 验证 ad-hoc 和登记证书共存，拒绝陌生签名与坏登记。
 * [POS]: 旧 remote-key addon 的宿主身份回归；新 adapter 门由同目录独立 fixture 验证。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <CommonCrypto/CommonDigest.h>
#import <sys/stat.h>
#import <unistd.h>

static NSString *ownedHome;
static NSDictionary *hostInfo;
static bool hostValid=true, moduleValid=true;
static NSString *fixtureHome(void) { return ownedHome; }
static OSStatus fixtureSelf(SecCSFlags flags, SecCodeRef *code) {
 *code=(SecCodeRef)CFBridgingRetain(@{}); return errSecSuccess;
}
static OSStatus fixtureInfo(SecCodeRef code, SecCSFlags flags, CFDictionaryRef *info) {
 *info=(CFDictionaryRef)CFBridgingRetain(hostInfo); return errSecSuccess;
}
static OSStatus fixtureHostValidity(SecCodeRef code, SecCSFlags flags, SecRequirementRef requirement) {
 return hostValid && flags==kSecCSStrictValidate ? errSecSuccess : errSecCSUnsigned;
}
static CFDataRef fixtureCertificate(SecCertificateRef certificate) {
 return (CFDataRef)CFRetain(certificate);
}
static OSStatus fixtureModule(CFURLRef url, SecCSFlags flags, SecStaticCodeRef *code) {
 *code=(SecStaticCodeRef)CFRetain(url); return errSecSuccess;
}
static OSStatus fixtureModuleValidity(SecStaticCodeRef code, SecCSFlags flags, SecRequirementRef requirement) {
 return moduleValid && requirement && flags==kSecCSStrictValidate ? errSecSuccess : errSecCSUnsigned;
}
#define NSHomeDirectory fixtureHome
#define SecCodeCopySelf fixtureSelf
#define SecCodeCopySigningInformation fixtureInfo
#define SecCodeCheckValidity fixtureHostValidity
#define SecCertificateCopyData fixtureCertificate
#define SecCertificateGetTypeID CFDataGetTypeID
#define SecStaticCodeCreateWithPath fixtureModule
#define SecStaticCodeCheckValidity fixtureModuleValidity
#import "remote-key-compat.m"
#undef NSHomeDirectory

static NSData *certificate(void) { return [@"owned synthetic leaf certificate" dataUsingEncoding:NSUTF8StringEncoding]; }
static NSString *digest(NSData *data, bool sha256) {
 unsigned char bytes[CC_SHA256_DIGEST_LENGTH];
 if(sha256) CC_SHA256(data.bytes,(CC_LONG)data.length,bytes); else CC_SHA1(data.bytes,(CC_LONG)data.length,bytes);
 NSMutableString *text=[NSMutableString string];
 for(size_t i=0;i<(sha256 ? CC_SHA256_DIGEST_LENGTH : CC_SHA1_DIGEST_LENGTH);i++) [text appendFormat:@"%02x",bytes[i]];
 return text;
}
static NSString *state(void) { return [ownedHome stringByAppendingPathComponent:@".incodex/macos-signing"]; }
static NSString *asset(NSString *name) { return [state() stringByAppendingPathComponent:name]; }
static void writePrivate(NSString *path, NSData *bytes) {
 NSCAssert([bytes writeToFile:path atomically:NO],@"write owned fixture"); chmod(path.fileSystemRepresentation,0600);
}
static NSMutableDictionary *registration(void) {
 NSString *storeHash=[@"a" stringByPaddingToLength:64 withString:@"a" startingAtIndex:0];
 return [@{@"schemaVersion":@1,@"keychainPath":asset(@"identity.keychain-db"),
   @"certificateSha1":digest(certificate(),false),@"certificateSha256":digest(certificate(),true),
   @"storePath":[ownedHome stringByAppendingPathComponent:[@".incodex/helpers/macos-signing/" stringByAppendingFormat:@"%@/incodex-signing-store",storeHash]],@"storeSha256":storeHash} mutableCopy];
}
static void writeRegistration(NSDictionary *value) {
 writePrivate(asset(@"identity.json"),[NSJSONSerialization dataWithJSONObject:value options:0 error:NULL]);
}
static void reset(void) {
 NSFileManager *files=NSFileManager.defaultManager;
 [files removeItemAtPath:[ownedHome stringByAppendingPathComponent:@".incodex"] error:NULL];
 NSCAssert([files createDirectoryAtPath:state() withIntermediateDirectories:YES attributes:@{NSFilePosixPermissions:@0700} error:NULL],@"create owned fixture");
 chmod([ownedHome stringByAppendingPathComponent:@".incodex"].fileSystemRepresentation,0700);
 writePrivate(asset(@"certificate.der"),certificate());writeRegistration(registration());
 hostInfo=@{(__bridge id)kSecCodeInfoIdentifier:@"com.openai.codex",(__bridge id)kSecCodeInfoFlags:@(kSecCodeSignatureRuntime),
   (__bridge id)kSecCodeInfoCertificates:@[certificate()]};
 hostValid=true;moduleValid=true;
}
static void hostField(CFStringRef key, id value) {
 NSMutableDictionary *changed=[hostInfo mutableCopy];changed[(__bridge id)key]=value;hostInfo=changed;
}
static int failures=0, checks=0;
static void check(NSString *name, bool expected) {
 NSString *path=[NSBundle.mainBundle.resourcePath stringByAppendingPathComponent:@"native/remote-control-device-key.node"];
 bool actual=trustedModule(path);checks++;
 if(actual!=expected) { fprintf(stderr,"FAIL: %s expected=%d actual=%d\n",name.UTF8String,expected,actual);failures++; }
}
int main(void) { @autoreleasepool {
 char temporary[PATH_MAX], canonical[PATH_MAX];
 snprintf(temporary,sizeof(temporary),"%s/incodex-remote-host-XXXXXX",NSTemporaryDirectory().fileSystemRepresentation);
 if(!mkdtemp(temporary) || !realpath(temporary,canonical)) return 2;
 ownedHome=[NSString stringWithUTF8String:canonical];
 @try {
  reset();check(@"registered local certificate enables compatibility without private signing assets",true);
  reset();hostField(kSecCodeInfoFlags,@(kSecCodeSignatureAdhoc));check(@"legacy ad-hoc remains enabled",true);
  [[NSFileManager defaultManager] removeItemAtPath:state() error:NULL];check(@"ad-hoc needs no local registration",true);
  reset();writePrivate(asset(@"identity.json"),[@"invalid" dataUsingEncoding:NSUTF8StringEncoding]);
  hostField(kSecCodeInfoFlags,@(kSecCodeSignatureAdhoc));check(@"corrupt local registration does not disable ad-hoc",true);
  reset();hostField(kSecCodeInfoIdentifier,@"unrelated.host");check(@"wrong host identifier",false);
  reset();hostValid=false;check(@"invalid host signature",false);
  reset();hostField(kSecCodeInfoCertificates,@[@"not a certificate"]);check(@"invalid certificate type",false);
  reset();hostField(kSecCodeInfoCertificates,@[]);check(@"missing host leaf certificate",false);
  reset();hostField(kSecCodeInfoCertificates,@[[@"other signer" dataUsingEncoding:NSUTF8StringEncoding]]);check(@"unregistered signer",false);
  reset();hostField(kSecCodeInfoCertificates,@[[@"other leaf" dataUsingEncoding:NSUTF8StringEncoding],certificate()]);check(@"registered certificate later in chain is not the leaf",false);
  reset();hostField(kSecCodeInfoTeamIdentifier,@"2DC432GLL2");check(@"official host is not a local fallback",false);
  reset();[[NSFileManager defaultManager] removeItemAtPath:asset(@"identity.json") error:NULL];check(@"missing registration",false);
  reset();writePrivate(asset(@"identity.json"),[@"invalid" dataUsingEncoding:NSUTF8StringEncoding]);check(@"malformed registration",false);
  reset();NSMutableDictionary *metadata=registration();metadata[@"schemaVersion"]=@2;writeRegistration(metadata);check(@"unknown schema",false);
  reset();metadata=registration();metadata[@"unknownField"]=@1;writeRegistration(metadata);check(@"unknown registration field",false);
  reset();metadata=registration();metadata[@"keychainPath"]=@"/unrelated/identity.keychain-db";writeRegistration(metadata);check(@"registration belongs to another identity directory",false);
  reset();metadata=registration();metadata[@"storeSha256"]=@64;writeRegistration(metadata);check(@"nonstring store hash",false);
  reset();metadata=registration();metadata[@"storeSha256"]=[@"z" stringByPaddingToLength:64 withString:@"z" startingAtIndex:0];writeRegistration(metadata);check(@"invalid store hash",false);
  reset();metadata=registration();metadata[@"storePath"]=@"/unrelated/incodex-signing-store";writeRegistration(metadata);check(@"foreign store path",false);
  reset();metadata=registration();metadata[@"certificateSha1"]=@"wrong";writeRegistration(metadata);check(@"wrong SHA1",false);
  reset();metadata=registration();metadata[@"certificateSha256"]=@"wrong";writeRegistration(metadata);check(@"wrong SHA256",false);
  reset();writePrivate(asset(@"certificate.der"),[@"changed" dataUsingEncoding:NSUTF8StringEncoding]);check(@"changed certificate bytes",false);
  reset();[[NSFileManager defaultManager] removeItemAtPath:asset(@"certificate.der") error:NULL];check(@"missing certificate",false);
  reset();chmod(asset(@"identity.json").fileSystemRepresentation,0644);check(@"nonprivate registration mode",false);
  reset();chmod(asset(@"certificate.der").fileSystemRepresentation,0644);check(@"nonprivate certificate mode",false);
  reset();NSString *outside=[ownedHome stringByAppendingPathComponent:@"outside-certificate"];
  writePrivate(outside,certificate());[[NSFileManager defaultManager] removeItemAtPath:asset(@"certificate.der") error:NULL];
  symlink(outside.fileSystemRepresentation,asset(@"certificate.der").fileSystemRepresentation);check(@"symlink certificate",false);
  reset();NSString *outsideRegistration=[ownedHome stringByAppendingPathComponent:@"outside-registration"];
  writePrivate(outsideRegistration,[NSJSONSerialization dataWithJSONObject:registration() options:0 error:NULL]);
  [[NSFileManager defaultManager] removeItemAtPath:asset(@"identity.json") error:NULL];
  symlink(outsideRegistration.fileSystemRepresentation,asset(@"identity.json").fileSystemRepresentation);check(@"symlink registration",false);
  reset();link(asset(@"certificate.der").fileSystemRepresentation,[ownedHome stringByAppendingPathComponent:@"linked-certificate"].fileSystemRepresentation);check(@"hardlinked certificate",false);
  reset();chmod(state().fileSystemRepresentation,0755);check(@"nonprivate signing directory",false);
  reset();NSString *saved=[ownedHome stringByAppendingPathComponent:@"redirected-signing"];
  [[NSFileManager defaultManager] moveItemAtPath:state() toPath:saved error:NULL];
  symlink(saved.fileSystemRepresentation,state().fileSystemRepresentation);check(@"symlink signing directory",false);
  reset();NSString *root=[ownedHome stringByAppendingPathComponent:@".incodex"];
  NSString *savedRoot=[ownedHome stringByAppendingPathComponent:@"redirected-root"];
  [[NSFileManager defaultManager] moveItemAtPath:root toPath:savedRoot error:NULL];
  symlink(savedRoot.fileSystemRepresentation,root.fileSystemRepresentation);check(@"symlink Incodex root",false);
  reset();moduleValid=false;check(@"invalid official module signature",false);
  reset();NSCAssert(!trustedModule(@"/unrelated/remote-control-device-key.node"),@"foreign module path remains excluded");checks++;
  printf("remote key host gate: %d checks, %d failures\n",checks,failures);
  return failures ? 1 : 0;
 } @finally { [[NSFileManager defaultManager] removeItemAtPath:ownedHome error:NULL]; }
} }
