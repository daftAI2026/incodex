/**
 * [INPUT]: 依赖私有固定消费者登记、有界 stdio 与默认 300 秒期限。
 * [OUTPUT]: 提供设备密钥消费者定位与异步 N-API 适配；夹具可注入短期限。
 * [POS]: 宿主仅分派请求；消费者独占私钥引用与系统授权。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#pragma once
#import "host-identity.h"
#include <poll.h>
#include <spawn.h>
#include <signal.h>
#include <sys/wait.h>
#include <time.h>
extern char **environ;

#ifndef INCODEX_DEVICE_HELPER_DEADLINE_SECONDS
#define INCODEX_DEVICE_HELPER_DEADLINE_SECONDS 300
#endif

// --- 登记与内容身份：沿现有只读私有文件规则，不探测钥匙串。 ---
static int incodexDeviceDirectory(NSString *home, NSArray<NSString *> *parts) {
 int fd=open(home.fileSystemRepresentation,O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
 struct stat st;
 if(fd<0)return -1;
 if(fstat(fd,&st)!=0 || st.st_uid!=geteuid() || (st.st_mode&0022)) {close(fd);return -1;}
 for(NSString *part in parts) {
  int child=openat(fd,part.fileSystemRepresentation,O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
  close(fd);fd=child;
  if(fd<0)return -1;
  if(fstat(fd,&st)!=0 || st.st_uid!=geteuid() || (st.st_mode&0777)!=0700) {close(fd);return -1;}
 }
 return fd;
}
static bool incodexDeviceHelperMatches(NSString *path, NSString *hash) {
 int fd=open(path.fileSystemRepresentation,O_RDONLY|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK);
 if(fd<0)return false;
 struct stat before,after;bool valid=false;
 if(fstat(fd,&before)==0 && S_ISREG(before.st_mode) && before.st_uid==geteuid() &&
    (before.st_mode&0777)==0700 && before.st_nlink==1 && before.st_size>0 && before.st_size<=1024*1024) {
  NSMutableData *data=[NSMutableData dataWithLength:(NSUInteger)before.st_size];size_t offset=0;
  while(offset<data.length) {
   ssize_t count=read(fd,(char *)data.mutableBytes+offset,data.length-offset);
   if(count<0 && errno==EINTR)continue;if(count<=0)break;offset+=(size_t)count;
  }
  char tail;ssize_t extra;do{extra=read(fd,&tail,1);}while(extra<0 && errno==EINTR);
  valid=offset==data.length && extra==0 && [incodexCertificateDigest(data,true) isEqual:hash] &&
   fstat(fd,&after)==0 && before.st_dev==after.st_dev && before.st_ino==after.st_ino &&
   before.st_size==after.st_size && before.st_mode==after.st_mode && before.st_uid==after.st_uid &&
   after.st_nlink==1 && before.st_mtimespec.tv_sec==after.st_mtimespec.tv_sec &&
   before.st_mtimespec.tv_nsec==after.st_mtimespec.tv_nsec;
 }
 close(fd);return valid;
}
static NSDictionary *incodexDeviceHelperAtHome(NSString *home) {
 int fd=incodexDeviceDirectory(home,@[@".incodex",@"macos-device-key"]);
 if(fd<0)return nil;
 NSData *body=incodexReadSigningFile(fd,"registration.json",4096);close(fd);
 id registration=body ? [NSJSONSerialization JSONObjectWithData:body options:0 error:NULL] : nil;
 if(![registration isKindOfClass:NSDictionary.class] || [registration count]!=3)return nil;
 for(NSString *field in @[@"schemaVersion",@"protocolVersion"]) {
  id number=registration[field];
  if(![number isKindOfClass:NSNumber.class] || CFGetTypeID((__bridge CFTypeRef)number)==CFBooleanGetTypeID() || ![number isEqual:@1])return nil;
 }
 NSString *hash=registration[@"helperSha256"];
 NSCharacterSet *nonHex=[[NSCharacterSet characterSetWithCharactersInString:@"0123456789abcdef"] invertedSet];
 if(![hash isKindOfClass:NSString.class] || hash.length!=64 || [hash rangeOfCharacterFromSet:nonHex].location!=NSNotFound)return nil;
 fd=incodexDeviceDirectory(home,@[@".incodex",@"helpers",@"macos-device-key",hash]);if(fd<0)return nil;close(fd);
 NSString *path=[home stringByAppendingPathComponent:[NSString stringWithFormat:@".incodex/helpers/macos-device-key/%@/incodex-device-key-helper",hash]];
 if(!incodexDeviceHelperMatches(path,hash))return nil;
 return @{@"path":path,@"sha256":hash};
}

// --- 每次请求一个短命子进程；一个截止时间覆盖写入、读取与退出。 ---
static double incodexDeviceClock(void) {struct timespec ts;clock_gettime(CLOCK_MONOTONIC,&ts);return ts.tv_sec+ts.tv_nsec/1e9;}
static void incodexDeviceReap(pid_t pid) {
 int status;if(waitpid(pid,&status,WNOHANG)==pid)return;
 kill(pid,SIGTERM);double until=incodexDeviceClock()+0.5;
 while(incodexDeviceClock()<until){pid_t found=waitpid(pid,&status,WNOHANG);if(found==pid || (found<0 && errno==ECHILD))return;usleep(10000);}
 kill(pid,SIGKILL);while(waitpid(pid,&status,0)<0 && errno==EINTR){}
}
static NSData *incodexRunDeviceHelper(NSDictionary *helper,NSData *request,NSTimeInterval timeout,int *failure) {
 *failure=1;
 NSString *path=helper[@"path"],*hash=helper[@"sha256"];
 if(![path isKindOfClass:NSString.class] || ![hash isKindOfClass:NSString.class] ||
    !incodexDeviceHelperMatches(path,hash) || request.length==0 || request.length>65536 || timeout<=0)return nil;
 int input[2]={-1,-1},output[2]={-1,-1};pid_t pid=-1;
 if(pipe(input)!=0 || pipe(output)!=0)goto finished;
 for(int i=0;i<2;i++){fcntl(input[i],F_SETFD,FD_CLOEXEC);fcntl(output[i],F_SETFD,FD_CLOEXEC);}
 posix_spawn_file_actions_t actions;posix_spawn_file_actions_init(&actions);
 posix_spawn_file_actions_adddup2(&actions,input[0],STDIN_FILENO);
 posix_spawn_file_actions_adddup2(&actions,output[1],STDOUT_FILENO);
 posix_spawn_file_actions_addopen(&actions,STDERR_FILENO,"/dev/null",O_WRONLY,0);
 posix_spawnattr_t attributes;posix_spawnattr_init(&attributes);
 posix_spawnattr_setflags(&attributes,POSIX_SPAWN_CLOEXEC_DEFAULT);
 const char *argv[]={path.fileSystemRepresentation,NULL};
 int spawned=posix_spawn(&pid,path.fileSystemRepresentation,&actions,&attributes,(char *const *)argv,environ);
 posix_spawnattr_destroy(&attributes);posix_spawn_file_actions_destroy(&actions);
 if(spawned!=0){pid=-1;*failure=2;goto finished;}
 close(input[0]);input[0]=-1;close(output[1]);output[1]=-1;
 fcntl(input[1],F_SETFL,fcntl(input[1],F_GETFL)|O_NONBLOCK);
 fcntl(output[0],F_SETFL,fcntl(output[0],F_GETFL)|O_NONBLOCK);
 // 防止已经退出的消费者将宿主变成 SIGPIPE 接收方。
 int noSignal=1;fcntl(input[1],F_SETNOSIGPIPE,noSignal);
 {
  NSMutableData *result=[NSMutableData data];size_t written=0;bool eof=false,exited=false;int status=0;
  double until=incodexDeviceClock()+timeout;
  while(!eof || !exited) {
   if(incodexDeviceClock()>=until){*failure=4;goto finished;}
   if(!exited){pid_t found=waitpid(pid,&status,WNOHANG);if(found==pid)exited=true;else if(found<0 && errno!=EINTR){*failure=3;goto finished;}}
   if(exited && (!WIFEXITED(status) || WEXITSTATUS(status)!=0)){pid=-1;*failure=5;goto finished;}
   if(input[1]>=0 && written==request.length){close(input[1]);input[1]=-1;}
   struct pollfd fds[2]={{.fd=output[0],.events=POLLIN},{.fd=input[1],.events=POLLOUT}};
   int polled=poll(fds,2,MIN(20,MAX(1,(int)((until-incodexDeviceClock())*1000))));
   if(polled<0 && errno==EINTR)continue;if(polled<0){*failure=3;goto finished;}
   if(input[1]>=0 && (fds[1].revents&POLLOUT)){
    ssize_t count=write(input[1],(const char *)request.bytes+written,request.length-written);
    if(count>0)written+=(size_t)count;else if(count<0 && errno!=EINTR && errno!=EAGAIN){*failure=3;goto finished;}
   }
   if(output[0]>=0 && (fds[0].revents&(POLLIN|POLLHUP|POLLERR))){
    unsigned char bytes[2048];ssize_t count=read(output[0],bytes,sizeof(bytes));
    if(count>0){if(result.length+(NSUInteger)count>16384){*failure=6;goto finished;}[result appendBytes:bytes length:(NSUInteger)count];}
    else if(count==0){eof=true;close(output[0]);output[0]=-1;}
    else if(errno!=EINTR && errno!=EAGAIN){*failure=3;goto finished;}
   }
   if(input[1]>=0 && (fds[1].revents&(POLLERR|POLLHUP)) && written<request.length){*failure=3;goto finished;}
  }
  pid=-1;
  if(result.length==0){*failure=6;goto finished;}
  for(int i=0;i<2;i++){if(input[i]>=0)close(input[i]);if(output[i]>=0)close(output[i]);}
  *failure=0;return result;
 }
finished:
 if(pid>0)incodexDeviceReap(pid);
 for(int i=0;i<2;i++){if(input[i]>=0)close(input[i]);if(output[i]>=0)close(output[i]);}
 return nil;
}

#ifndef INCODEX_DEVICE_TRANSPORT_TEST
// --- N-API 只在主线程操作 JS；工作线程只执行有界消费者协议。 ---
static NSDictionary *preparedDeviceHelper;
typedef struct {
 napi_async_work work;napi_deferred deferred;CFDataRef request;CFDataRef output;
 CFStringRef keyId;bool publicQuery;int failure;
} IncodexDeviceWork;
static void incodexRejectDevice(napi_env env,napi_deferred deferred,OSStatus status) {
 napi_value message,error,value;
 napi_create_string_utf8(env,"Cannot access the existing secure device key.",NAPI_AUTO_LENGTH,&message);
 napi_create_error(env,NULL,message,&error);
 const char *code=status==errSecItemNotFound ? "remote_control_device_key_unusable" : "remote_control_device_key_store_unavailable";
 napi_create_string_utf8(env,code,NAPI_AUTO_LENGTH,&value);napi_set_named_property(env,error,"errorCode",value);
 napi_create_int32(env,status,&value);napi_set_named_property(env,error,"osStatus",value);
 napi_reject_deferred(env,deferred,error);
}
static napi_value prepareDeviceKeyAccess(napi_env env,napi_callback_info info) {
 (void)info;bool ready=false;
 @autoreleasepool {
  if(deviceKeyBridgeHostReady()){
   NSDictionary *candidate=incodexDeviceHelperAtHome(NSHomeDirectory());
   if(candidate && (!preparedDeviceHelper || [preparedDeviceHelper isEqual:candidate])){if(!preparedDeviceHelper)preparedDeviceHelper=candidate;ready=true;}
  }
 }
 napi_value result;napi_get_boolean(env,ready,&result);return result;
}
static void executeDeviceWork(napi_env env,void *raw) {
 (void)env;IncodexDeviceWork *work=raw;
 @autoreleasepool {
  NSDictionary *current=incodexDeviceHelperAtHome(NSHomeDirectory());
  if(!current || ![current isEqual:preparedDeviceHelper]){work->failure=1;return;}
  NSData *data=incodexRunDeviceHelper(current,(__bridge NSData *)work->request,INCODEX_DEVICE_HELPER_DEADLINE_SECONDS,&work->failure);
  if(data)work->output=CFBridgingRetain(data);
 }
}
static void completeDeviceWork(napi_env env,napi_status completion,void *raw) {
 IncodexDeviceWork *work=raw;
 @autoreleasepool {
  id reply=work->output ? [NSJSONSerialization JSONObjectWithData:(__bridge NSData *)work->output options:0 error:NULL] : nil;
  if(![reply isKindOfClass:NSDictionary.class])reply=nil;
  id rawResult=reply[@"result"];
  NSDictionary *result=[rawResult isKindOfClass:NSDictionary.class] ? rawResult : nil;
  bool ok=completion==napi_ok && !work->failure && [reply[@"ok"] isEqual:@YES] &&
   [result isKindOfClass:NSDictionary.class] && [result[@"algorithm"] isEqual:@"ecdsa_p256_sha256"];
  NSArray *fields=work->publicQuery ? @[@"keyId",@"algorithm",@"protectionClass",@"publicKeySpkiDerBase64"] : @[@"algorithm",@"signatureDerBase64"];
  if(ok && work->publicQuery)ok=[result[@"keyId"] isEqual:(__bridge NSString *)work->keyId] && [result[@"protectionClass"] isEqual:@"os_protected_nonextractable"];
  for(NSString *field in fields)if(![result[field] isKindOfClass:NSString.class] || [result[field] length]>16384)ok=false;
  if(ok){
   napi_value object;napi_create_object(env,&object);
   for(NSString *field in fields){napi_value value;napi_create_string_utf8(env,[result[field] UTF8String],NAPI_AUTO_LENGTH,&value);napi_set_named_property(env,object,field.UTF8String,value);}
   napi_resolve_deferred(env,work->deferred,object);
  }else{
   id status=[reply isKindOfClass:NSDictionary.class] ? reply[@"status"] : nil;
   OSStatus error=[status isKindOfClass:NSNumber.class] ? [status intValue] : errSecNotAvailable;
   incodexRejectDevice(env,work->deferred,error);
  }
 }
 if(work->request)CFRelease(work->request);if(work->output)CFRelease(work->output);if(work->keyId)CFRelease(work->keyId);
 napi_delete_async_work(env,work->work);free(work);
}
static napi_value dispatchDeviceKey(napi_env env,napi_callback_info info,bool publicQuery) {
 napi_deferred deferred;napi_value promise;napi_create_promise(env,&deferred,&promise);
 @autoreleasepool {
  napi_value args[3];size_t argc=3;char key[128];size_t size=0;
  if(!deviceKeyBridgeHostReady() || !preparedDeviceHelper || napi_get_cb_info(env,info,&argc,args,NULL,NULL)!=napi_ok ||
     argc!=(publicQuery?1:2) || napi_get_value_string_utf8(env,args[0],key,sizeof(key),&size)!=napi_ok || size!=50){
   incodexRejectDevice(env,deferred,errSecParam);return promise;
  }
  NSString *keyId=[[NSString alloc]initWithBytes:key length:size encoding:NSUTF8StringEncoding];
  if(!keyId || ![keyId hasPrefix:@"dk_osn_"]){incodexRejectDevice(env,deferred,errSecParam);return promise;}
  NSMutableDictionary *request=[@{@"schemaVersion":@1,@"operation":publicQuery?@"public":@"sign",@"keyId":keyId} mutableCopy];
  if(!publicQuery){bool buffer=false;void *bytes=NULL;size_t length=0;
   if(napi_is_buffer(env,args[1],&buffer)!=napi_ok || !buffer || napi_get_buffer_info(env,args[1],&bytes,&length)!=napi_ok || length==0 || length>45000){incodexRejectDevice(env,deferred,errSecParam);return promise;}
   request[@"payloadBase64"]=[[NSData dataWithBytes:bytes length:length] base64EncodedStringWithOptions:0];
  }
  NSData *body=[NSJSONSerialization dataWithJSONObject:request options:0 error:NULL];
  if(!body || body.length>65536){incodexRejectDevice(env,deferred,errSecParam);return promise;}
  IncodexDeviceWork *work=calloc(1,sizeof(*work));
  if(!work){incodexRejectDevice(env,deferred,errSecAllocate);return promise;}
  work->deferred=deferred;work->request=CFBridgingRetain(body);work->keyId=CFBridgingRetain(keyId);work->publicQuery=publicQuery;
  napi_value name;napi_create_string_utf8(env,"incodex-device-key",NAPI_AUTO_LENGTH,&name);
  if(napi_create_async_work(env,NULL,name,executeDeviceWork,completeDeviceWork,work,&work->work)!=napi_ok || napi_queue_async_work(env,work->work)!=napi_ok){
   if(work->work)napi_delete_async_work(env,work->work);CFRelease(work->request);CFRelease(work->keyId);free(work);incodexRejectDevice(env,deferred,errSecNotAvailable);
  }
 }
 return promise;
}
static napi_value getDeviceKeyPublic(napi_env env,napi_callback_info info){return dispatchDeviceKey(env,info,true);}
static napi_value signDeviceKey(napi_env env,napi_callback_info info){return dispatchDeviceKey(env,info,false);}
#endif
