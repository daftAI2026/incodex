/**
 * [INPUT]: 依赖真实 N-API 桥、合成登记与短命响应程序。
 * [OUTPUT]: 验证真实 Promise、Buffer 编码及官方错误码。
 * [POS]: 端到端传输夹具；不调用宿主或任何 Keychain API。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#if defined(INCODEX_DEVICE_BRIDGE_HELPER_FIXTURE)
#include <unistd.h>
int main(void){@autoreleasepool{
 NSMutableData *data=[NSMutableData data];unsigned char bytes[1024];ssize_t count;
 while((count=read(0,bytes,sizeof(bytes)))>0){[data appendBytes:bytes length:count];if(data.length>65536)return 7;}
 id request=[NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
 if(![request isKindOfClass:NSDictionary.class] || ![request[@"schemaVersion"] isEqual:@1])return 7;
 NSDictionary *reply;
 if([request[@"operation"] isEqual:@"public"]){
  reply=@{@"ok":@YES,@"result":@{@"keyId":request[@"keyId"],@"algorithm":@"ecdsa_p256_sha256",
   @"protectionClass":@"os_protected_nonextractable",@"publicKeySpkiDerBase64":@"synthetic-public-result"}};
 }else{
  NSData *expected=[@"synthetic-buffer-challenge" dataUsingEncoding:NSUTF8StringEncoding];
  if(![request[@"operation"] isEqual:@"sign"] || ![request[@"payloadBase64"] isEqual:[expected base64EncodedStringWithOptions:0]])return 7;
  reply=@{@"ok":@NO,@"status":@(-25293)};
 }
 NSData *body=[NSJSONSerialization dataWithJSONObject:reply options:0 error:NULL];
 return write(1,body.bytes,body.length)==(ssize_t)body.length ? 0:7;
}}
#else
#import "vendor/node/node_api.h"
static NSString *fixtureDeviceHome(void){return @INCODEX_DEVICE_NAPI_FIXTURE_HOME;}
static bool deviceKeyBridgeHostReady(void){return true;}
#define NSHomeDirectory fixtureDeviceHome
#import "device-key-bridge.h"
#undef NSHomeDirectory
static napi_value initialize(napi_env env,napi_value exports){
 napi_value function;
 napi_create_function(env,"prepareDeviceKeyAccess",NAPI_AUTO_LENGTH,prepareDeviceKeyAccess,NULL,&function);napi_set_named_property(env,exports,"prepareDeviceKeyAccess",function);
 napi_create_function(env,"getDeviceKeyPublic",NAPI_AUTO_LENGTH,getDeviceKeyPublic,NULL,&function);napi_set_named_property(env,exports,"getDeviceKeyPublic",function);
 napi_create_function(env,"signDeviceKey",NAPI_AUTO_LENGTH,signDeviceKey,NULL,&function);napi_set_named_property(env,exports,"signDeviceKey",function);
 return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME,initialize)
#endif
