/**
 * [INPUT]: 依赖真实 Security 签名 API 与测试传入的私有临时目录。
 * [OUTPUT]: 输出当前合成宿主的三态身份，不调用密钥或权限 API。
 * [POS]: 宿主身份实签回归；只签临时可执行文件，不进入官方包。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <Security/Security.h>

static NSString *ownedHome;
static NSString *fixtureHome(void) { return ownedHome; }
#define NSHomeDirectory fixtureHome
#import "host-identity.h"
#undef NSHomeDirectory

int main(int argc, const char *argv[]) { @autoreleasepool {
 if(argc!=2) return 64;
 ownedHome=[NSString stringWithUTF8String:argv[1]];
 switch(incodexHostIdentity()) {
  case IncodexHostIdentityRegisteredLocal: puts("registered-local"); break;
  case IncodexHostIdentityAdhoc: puts("ad-hoc"); break;
  default: puts("unsupported"); break;
 }
 return 0;
} }
