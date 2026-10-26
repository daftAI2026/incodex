/**
 * [INPUT]: 依赖合成宿主身份与 Security 模块签名结果。
 * [OUTPUT]: 验证新 adapter 的宿主门和官方模块门。
 * [POS]: 纯原生信任门回归；不读取 Keychain 或真实宿主身份。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <limits.h>
#import <libproc.h>
#import <unistd.h>
#import "host-identity.h"

static IncodexHostIdentity fixtureIdentity=IncodexHostIdentityRegisteredLocal;
static bool fixturePathValid=true;
static bool fixtureSignatureValid=true;
static NSString *fixtureResources;

@interface DeviceAdapterTestBundle : NSObject
@property(nonatomic,copy) NSString *resourcePath;
+ (instancetype)mainBundle;
@end

@implementation DeviceAdapterTestBundle
+ (instancetype)mainBundle {
 static DeviceAdapterTestBundle *bundle;
 if(!bundle)bundle=[DeviceAdapterTestBundle new];
 bundle.resourcePath=fixtureResources;
 return bundle;
}
@end

static IncodexHostIdentity fixtureHostIdentity(void) { return fixtureIdentity; }
static int fixtureProcPidpath(int pid,void *buffer,uint32_t size) {
 (void)pid;
 return snprintf(buffer,size,"/Applications/ChatGPT.app/Contents/MacOS/ChatGPT");
}
static OSStatus fixtureStaticCodeCreate(CFURLRef url,SecCSFlags flags,SecStaticCodeRef *code) {
 (void)flags;
 NSString *path=[(__bridge NSURL *)url path];
 NSString *expected=[fixtureResources stringByAppendingPathComponent:@"native/remote-control-device-key.node"];
 if(!fixturePathValid || ![path isEqual:expected])return errSecCSUnsigned;
 *code=(SecStaticCodeRef)CFBridgingRetain([NSObject new]);
 return errSecSuccess;
}
static OSStatus fixtureRequirementCreate(CFStringRef text,SecCSFlags flags,SecRequirementRef *requirement) {
 (void)flags;
 NSString *rule=(__bridge NSString *)text;
 if(![rule containsString:@"certificate leaf[subject.OU] = \"2DC432GLL2\""])return errSecCSReqFailed;
 *requirement=(SecRequirementRef)CFBridgingRetain([NSObject new]);
 return errSecSuccess;
}
static OSStatus fixtureStaticCodeCheck(SecStaticCodeRef code,SecCSFlags flags,SecRequirementRef requirement) {
 (void)code;(void)requirement;
 return fixtureSignatureValid && flags==kSecCSStrictValidate ? errSecSuccess:errSecCSUnsigned;
}
static int checks=0,failures=0;
static bool trustedOfficialRemoteKeyModule(void);
static void check(NSString *name,bool expected) {
 bool actual=trustedOfficialRemoteKeyModule();checks++;
 if(actual!=expected){fprintf(stderr,"FAIL: %s expected=%d actual=%d\n",name.UTF8String,expected,actual);failures++;}
}

#define INCODEX_DEVICE_TRANSPORT_TEST 1
#define NSBundle DeviceAdapterTestBundle
#define incodexHostIdentity fixtureHostIdentity
#define proc_pidpath fixtureProcPidpath
#define SecStaticCodeCreateWithPath fixtureStaticCodeCreate
#define SecRequirementCreateWithString fixtureRequirementCreate
#define SecStaticCodeCheckValidity fixtureStaticCodeCheck
#import "device-key-adapter.m"
#undef SecStaticCodeCheckValidity
#undef SecRequirementCreateWithString
#undef SecStaticCodeCreateWithPath
#undef proc_pidpath
#undef incodexHostIdentity
#undef NSBundle

int main(void) {@autoreleasepool {
 char temporary[PATH_MAX],canonical[PATH_MAX];
 snprintf(temporary,sizeof(temporary),"%s/incodex-device-adapter-host-XXXXXX",NSTemporaryDirectory().fileSystemRepresentation);
 if(!mkdtemp(temporary) || !realpath(temporary,canonical))return 2;
 NSString *directory=[[NSString stringWithUTF8String:canonical] stringByResolvingSymlinksInPath];
 fixtureResources=[directory stringByAppendingPathComponent:@"Resources"];
 NSString *module=[fixtureResources stringByAppendingPathComponent:@"native/remote-control-device-key.node"];
 [[NSFileManager defaultManager] createDirectoryAtPath:module.stringByDeletingLastPathComponent withIntermediateDirectories:YES attributes:@{NSFilePosixPermissions:@0700} error:NULL];
 [@"synthetic signed module" writeToFile:module atomically:NO encoding:NSUTF8StringEncoding error:NULL];
 @try {
  check(@"registered local host and official Apple signature",true);
  fixtureIdentity=IncodexHostIdentityAdhoc;check(@"ad-hoc host is excluded",false);
  fixtureIdentity=IncodexHostIdentityRegisteredLocal;
  fixturePathValid=false;check(@"module outside the exact bundle path is excluded",false);
  fixturePathValid=true;fixtureSignatureValid=false;check(@"unsigned official module is excluded",false);
  fixtureSignatureValid=true;
  NSString *redirect=[directory stringByAppendingPathComponent:@"redirected-resources"];
  [[NSFileManager defaultManager] moveItemAtPath:fixtureResources toPath:redirect error:NULL];
  symlink(redirect.fileSystemRepresentation,fixtureResources.fileSystemRepresentation);
  check(@"symlinked resource path is excluded",false);
  fixtureResources=redirect;
  check(@"registered local host prepares without fishhook state",true);
  printf("device-key adapter trust gate: %d checks, %d failures\n",checks,failures);
 } @finally {
  [[NSFileManager defaultManager] removeItemAtPath:directory error:NULL];
 }
 return failures?1:0;
}}
