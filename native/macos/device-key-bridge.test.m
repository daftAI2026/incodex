/**
 * [INPUT]: 依赖合成 helper、私有临时登记、取消 fd 与机器期限。
 * [OUTPUT]: 验证身份读取、事件等待、输出上限、期限与损坏拒绝。
 * [POS]: 固定消费者传输回归；不访问任何钥匙串或宿主。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#define INCODEX_DEVICE_TRANSPORT_TEST 1
#import "device-key-bridge.h"
#import <CommonCrypto/CommonDigest.h>
#include <sys/stat.h>
static NSString *digest(NSData *data) {
 unsigned char out[CC_SHA256_DIGEST_LENGTH];CC_SHA256(data.bytes,(CC_LONG)data.length,out);
 NSMutableString *value=[NSMutableString string];for(size_t i=0;i<sizeof(out);i++)[value appendFormat:@"%02x",out[i]];return value;
}
static void save(NSString *path,NSData *bytes,mode_t mode) {
 [[NSFileManager defaultManager] createDirectoryAtPath:path.stringByDeletingLastPathComponent withIntermediateDirectories:YES attributes:@{NSFilePosixPermissions:@0700} error:NULL];
 [bytes writeToFile:path atomically:NO];chmod(path.fileSystemRepresentation,mode);
}
static void check(bool value,const char *message) {if(!value){fprintf(stderr,"FAIL: %s\n",message);exit(1);}}
int main(int argc,char **argv) {@autoreleasepool {
 if(argc!=2)return 64;NSString *home=@(argv[1]);
 NSString *source=@"#!/bin/sh\nwhile IFS= read -r line; do :; done\nprintf '{\"ok\":true,\"result\":{}}'\n";
 NSData *bytes=[source dataUsingEncoding:NSUTF8StringEncoding];NSString *hash=digest(bytes);
 NSString *helperPath=[home stringByAppendingPathComponent:[NSString stringWithFormat:@".incodex/helpers/macos-device-key/%@/incodex-device-key-helper",hash]];
 save(helperPath,bytes,0700);
 NSString *regPath=[home stringByAppendingPathComponent:@".incodex/macos-device-key/registration.json"];
 NSDictionary *reg=@{@"schemaVersion":@1,@"protocolVersion":@1,@"helperSha256":hash};
 save(regPath,[NSJSONSerialization dataWithJSONObject:reg options:0 error:NULL],0600);
 NSDictionary *helper=incodexDeviceHelperAtHome(home);
 check(helper!=nil,"private registered helper resolves");
 int failure=0;NSData *request=[@"{}" dataUsingEncoding:NSUTF8StringEncoding];
 NSData *response=incodexRunDeviceHelper(helper,request,1,false,-1,&failure);
 check(response && failure==0,"bounded child response completes");
 check([[[NSString alloc]initWithData:response encoding:NSUTF8StringEncoding] isEqual:@"{\"ok\":true,\"result\":{}}"],"exact response retained");
 chmod(regPath.fileSystemRepresentation,0644);check(incodexDeviceHelperAtHome(home)==nil,"nonprivate registration rejected");chmod(regPath.fileSystemRepresentation,0600);
 save(helperPath,[@"changed" dataUsingEncoding:NSUTF8StringEncoding],0700);check(incodexDeviceHelperAtHome(home)==nil,"changed helper rejected");
 save(helperPath,bytes,0700);
 NSMutableDictionary *unknown=[reg mutableCopy];unknown[@"protocolVersion"]=@2;
 save(regPath,[NSJSONSerialization dataWithJSONObject:unknown options:0 error:NULL],0600);check(incodexDeviceHelperAtHome(home)==nil,"unknown protocol rejected");
 save(regPath,[NSJSONSerialization dataWithJSONObject:reg options:0 error:NULL],0600);
 // --- 真正运行截止、异常退出和超量输出，不用源码字符串冒充边界。 ---
 NSArray *programs=@[@"#!/bin/sh\nexec /bin/sleep 5\n",@"#!/bin/sh\nexit 3\n",@"#!/bin/sh\ni=0; while [ $i -lt 17000 ]; do printf x; i=$((i+1)); done\n"];
 NSArray *expectedFailures=@[@4,@5,@6];
 NSArray *deadlines=@[@0.1,@2.0,@2.0];
 for(NSUInteger index=0;index<programs.count;index++) {
  NSString *program=programs[index];
  NSData *body=[program dataUsingEncoding:NSUTF8StringEncoding];NSString *address=digest(body);
  NSString *target=[home stringByAppendingPathComponent:[NSString stringWithFormat:@".incodex/helpers/macos-device-key/%@/incodex-device-key-helper",address]];
  save(target,body,0700);
  double started=incodexDeviceClock();
  NSData *output=incodexRunDeviceHelper(@{@"path":target,@"sha256":address},request,[deadlines[index] doubleValue],false,-1,&failure);
  int expected=[expectedFailures[index] intValue];
  if(output || failure!=expected){fprintf(stderr,"FAIL: child fixture %lu returned failure %d, expected %d\n",(unsigned long)index,failure,expected);exit(1);}
  check(incodexDeviceClock()-started<3,"request deadline bounds owned child cleanup");
 }
 [[NSFileManager defaultManager] removeItemAtPath:regPath error:NULL];symlink(helperPath.fileSystemRepresentation,regPath.fileSystemRepresentation);
 check(incodexDeviceHelperAtHome(home)==nil,"registration symlink rejected");
 puts("device helper transport contracts passed");
 }return 0;}
