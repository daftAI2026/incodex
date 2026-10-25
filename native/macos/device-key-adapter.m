/**
 * [INPUT]: 依赖登记宿主身份、官方远控模块签名与固定 helper bridge。
 * [OUTPUT]: 导出 prepare、public、sign 三个 N-API 方法。
 * [POS]: 旧宿主的热 adapter；不创建密钥、不加载 SecKey fishhook。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <libproc.h>
#import <limits.h>
#import "host-identity.h"
#import "vendor/node/node_api.h"

static bool deviceKeyBridgeHostReady(void) {
 char executable[PROC_PIDPATHINFO_MAXSIZE]={0};
 return incodexHostIdentity()==IncodexHostIdentityRegisteredLocal &&
  proc_pidpath(getpid(),executable,sizeof(executable))>0 &&
  strcmp(executable,"/Applications/ChatGPT.app/Contents/MacOS/ChatGPT")==0;
}

#define prepareDeviceKeyAccess prepareRegisteredDeviceKeyHelper
#import "device-key-bridge.h"
#undef prepareDeviceKeyAccess

static NSString *officialRemoteKeyModulePath(void) {
 return [NSBundle.mainBundle.resourcePath stringByAppendingPathComponent:@"native/remote-control-device-key.node"];
}

static bool trustedOfficialRemoteKeyModule(void) {
 NSString *path=officialRemoteKeyModulePath();
 if(!path || ![path isEqualToString:path.stringByResolvingSymlinksInPath] ||
    !deviceKeyBridgeHostReady())return false;
 SecStaticCodeRef module=NULL;SecRequirementRef requirement=NULL;
 NSString *rule=@"anchor apple generic and certificate leaf[subject.OU] = \"2DC432GLL2\"";
 bool valid=SecStaticCodeCreateWithPath((__bridge CFURLRef)[NSURL fileURLWithPath:path],kSecCSDefaultFlags,&module)==errSecSuccess &&
  SecRequirementCreateWithString((__bridge CFStringRef)rule,kSecCSDefaultFlags,&requirement)==errSecSuccess &&
  SecStaticCodeCheckValidity(module,kSecCSStrictValidate,requirement)==errSecSuccess;
 if(requirement)CFRelease(requirement);if(module)CFRelease(module);
 return valid;
}

#ifndef INCODEX_DEVICE_TRANSPORT_TEST
static napi_value prepareDeviceKeyAccess(napi_env env,napi_callback_info info) {
 size_t argc=1;napi_value argument;
 if(napi_get_cb_info(env,info,&argc,&argument,NULL,NULL)!=napi_ok || argc!=0 ||
    !trustedOfficialRemoteKeyModule()) {
  napi_value unavailable;napi_get_boolean(env,false,&unavailable);return unavailable;
 }
 return prepareRegisteredDeviceKeyHelper(env,info);
}

static napi_value initialize(napi_env env,napi_value exports) {
 napi_value function;
 napi_create_function(env,"prepareDeviceKeyAccess",NAPI_AUTO_LENGTH,prepareDeviceKeyAccess,NULL,&function);
 napi_set_named_property(env,exports,"prepareDeviceKeyAccess",function);
 napi_create_function(env,"getDeviceKeyPublic",NAPI_AUTO_LENGTH,getDeviceKeyPublic,NULL,&function);
 napi_set_named_property(env,exports,"getDeviceKeyPublic",function);
 napi_create_function(env,"signDeviceKey",NAPI_AUTO_LENGTH,signDeviceKey,NULL,&function);
 napi_set_named_property(env,exports,"signDeviceKey",function);
 return exports;
}

NAPI_MODULE(NODE_GYP_MODULE_NAME,initialize)
#endif
