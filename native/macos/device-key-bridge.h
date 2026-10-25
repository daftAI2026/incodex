/**
 * [INPUT]: 依赖私有固定消费者登记、stdio、kqueue 与环境取消管道。
 * [OUTPUT]: 公钥全程限 300 秒；签名 stdin EOF 前限 300 秒。
 * [POS]: 专用 pthread 等待 helper；签名 EOF 后等响应、退出或取消，不占 libuv 池。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#pragma once
#import "host-identity.h"
#include <fcntl.h>
#include <poll.h>
#include <pthread.h>
#include <spawn.h>
#include <signal.h>
#include <sys/event.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>
#include <stdlib.h>
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
static bool incodexDeviceCancelPending(int cancelFd) {
 if(cancelFd<0)return false;
 struct pollfd event={.fd=cancelFd,.events=POLLIN};
 int ready=poll(&event,1,0);
 return ready>0 && (event.revents&(POLLIN|POLLHUP|POLLERR));
}
static bool incodexDeviceChildSucceeded(int status) {
 return WIFEXITED(status) && WEXITSTATUS(status)==0;
}
static NSData *incodexRunDeviceHelper(NSDictionary *helper,NSData *request,NSTimeInterval timeout,
                                     bool waitForApproval,int cancelFd,int *failure) {
 *failure=1;
 NSString *path=helper[@"path"],*hash=helper[@"sha256"];
 double until=incodexDeviceClock()+timeout;
 if(![path isKindOfClass:NSString.class] || ![hash isKindOfClass:NSString.class] ||
    request.length==0 || request.length>65536 || timeout<=0)return nil;
 if(incodexDeviceCancelPending(cancelFd)){*failure=7;return nil;}
 if(!incodexDeviceHelperMatches(path,hash)){*failure=1;return nil;}
 if(incodexDeviceClock()>=until){*failure=4;return nil;}
 int input[2]={-1,-1},output[2]={-1,-1};pid_t pid=-1;int queue=-1;
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
 int inputFlags=fcntl(input[1],F_GETFL),outputFlags=fcntl(output[0],F_GETFL);
 if(inputFlags<0 || outputFlags<0 || fcntl(input[1],F_SETFL,inputFlags|O_NONBLOCK)<0 ||
    fcntl(output[0],F_SETFL,outputFlags|O_NONBLOCK)<0){*failure=3;goto finished;}
 // 防止已经退出的消费者将宿主变成 SIGPIPE 接收方。
 int noSignal=1;if(fcntl(input[1],F_SETNOSIGPIPE,noSignal)<0){*failure=3;goto finished;}
 {
  NSMutableData *result=[NSMutableData data];size_t written=0;bool eof=false,exited=false,inputClosed=false;int status=0;
  queue=kqueue();if(queue<0){*failure=3;goto finished;}
  if(fcntl(queue,F_SETFD,FD_CLOEXEC)<0){*failure=3;goto finished;}
  struct kevent changes[4];int changeCount=0;
  EV_SET(&changes[changeCount++],(uintptr_t)output[0],EVFILT_READ,EV_ADD|EV_CLEAR,0,0,NULL);
  EV_SET(&changes[changeCount++],(uintptr_t)input[1],EVFILT_WRITE,EV_ADD|EV_CLEAR,0,0,NULL);
  if(cancelFd>=0)EV_SET(&changes[changeCount++],(uintptr_t)cancelFd,EVFILT_READ,EV_ADD|EV_CLEAR,0,0,NULL);
  pid_t found;do{found=waitpid(pid,&status,WNOHANG);}while(found<0 && errno==EINTR);
  if(found==pid){exited=true;pid=-1;if(!incodexDeviceChildSucceeded(status)){*failure=5;goto finished;}}
  else if(found==0)EV_SET(&changes[changeCount++],(uintptr_t)pid,EVFILT_PROC,EV_ADD|EV_ONESHOT,NOTE_EXIT,0,NULL);
  else {*failure=3;goto finished;}
  if(kevent(queue,changes,changeCount,NULL,0,NULL)<0){
   if(errno!=ESRCH || exited){*failure=3;goto finished;}
   do{found=waitpid(pid,&status,WNOHANG);}while(found<0 && errno==EINTR);
   if(found!=pid){*failure=3;goto finished;}
   exited=true;pid=-1;if(!incodexDeviceChildSucceeded(status)){*failure=5;goto finished;}
  }
  while(!eof || !exited) {
   if(incodexDeviceCancelPending(cancelFd)){*failure=7;goto finished;}
   bool deadlineActive=!waitForApproval || !inputClosed;
   struct timespec remaining,*wait=NULL;
   if(deadlineActive){
    double seconds=until-incodexDeviceClock();if(seconds<=0){*failure=4;goto finished;}
    remaining.tv_sec=(time_t)seconds;remaining.tv_nsec=(long)((seconds-(double)remaining.tv_sec)*1e9);wait=&remaining;
   }
   struct kevent events[4];int received=kevent(queue,NULL,0,events,4,wait);
   if(received<0 && errno==EINTR)continue;if(received<0){*failure=3;goto finished;}
   if(received==0){*failure=4;goto finished;}
   for(int i=0;i<received;i++){
    struct kevent *event=&events[i];
    if(cancelFd>=0 && event->filter==EVFILT_READ && event->ident==(uintptr_t)cancelFd){*failure=7;goto finished;}
    if(event->filter==EVFILT_PROC && event->ident==(uintptr_t)pid && (event->fflags&NOTE_EXIT)){
     pid_t found;do{found=waitpid(pid,&status,0);}while(found<0 && errno==EINTR);
     if(found!=pid){*failure=3;goto finished;}pid=-1;exited=true;
     if(!incodexDeviceChildSucceeded(status)){*failure=5;goto finished;}
    }else if(event->filter==EVFILT_WRITE && event->ident==(uintptr_t)input[1]){
     while(written<request.length){
      ssize_t count=write(input[1],(const char *)request.bytes+written,request.length-written);
      if(count>0)written+=(size_t)count;
      else if(count<0 && errno==EINTR)continue;
      else if(count<0 && (errno==EAGAIN || errno==EWOULDBLOCK))break;
      else {*failure=3;goto finished;}
     }
     if(written==request.length){
      kevent(queue,&(struct kevent){.ident=(uintptr_t)input[1],.filter=EVFILT_WRITE,.flags=EV_DELETE},1,NULL,0,NULL);
      close(input[1]);input[1]=-1;inputClosed=true;
     }
    }else if(event->filter==EVFILT_READ && event->ident==(uintptr_t)output[0]){
     unsigned char bytes[2048];
     for(;;){
      ssize_t count=read(output[0],bytes,sizeof(bytes));
      if(count>0){if(result.length+(NSUInteger)count>16384){*failure=6;goto finished;}[result appendBytes:bytes length:(NSUInteger)count];continue;}
      if(count==0){
       eof=true;kevent(queue,&(struct kevent){.ident=(uintptr_t)output[0],.filter=EVFILT_READ,.flags=EV_DELETE},1,NULL,0,NULL);
       close(output[0]);output[0]=-1;break;
      }
      if(errno==EINTR)continue;if(errno==EAGAIN || errno==EWOULDBLOCK)break;*failure=3;goto finished;
     }
    }
   }
  }
  if(result.length==0){*failure=6;goto finished;}
  if(queue>=0){close(queue);queue=-1;}
  for(int i=0;i<2;i++){if(input[i]>=0)close(input[i]);if(output[i]>=0)close(output[i]);}
  *failure=0;return result;
 }
finished:
 if(pid>0)incodexDeviceReap(pid);
 if(queue>=0)close(queue);
 for(int i=0;i<2;i++){if(input[i]>=0)close(input[i]);if(output[i]>=0)close(output[i]);}
 return nil;
}

#ifndef INCODEX_DEVICE_TRANSPORT_TEST
// --- N-API 只分派请求；专用 pthread 等待 helper，不占 libuv 池。 ---
static NSDictionary *preparedDeviceHelper;
typedef struct IncodexDeviceEnvironment IncodexDeviceEnvironment;
typedef struct IncodexDeviceFlight IncodexDeviceFlight;
typedef struct {
 napi_deferred deferred;CFDataRef output;CFStringRef keyId;
 IncodexDeviceFlight *flight;bool publicQuery;int failure;
} IncodexDeviceCompletion;
struct IncodexDeviceFlight {
 IncodexDeviceEnvironment *environment;IncodexDeviceFlight *next;
 napi_threadsafe_function completionFunction;pthread_t thread;
 IncodexDeviceCompletion *completion;CFDataRef request;CFStringRef keyId;
 // --- true 表示 worker 已结束 TSFN 计数责任。 ---
 bool publicQuery,threadStarted,threadJoined,threadsafeFinished,linked;
 int cancelPipe[2];
};
struct IncodexDeviceEnvironment {
 napi_env env;pthread_mutex_t mutex;size_t references;bool closing;
 IncodexDeviceFlight *flights;IncodexDeviceEnvironment *next;
};
static pthread_mutex_t incodexDeviceEnvironmentsMutex=PTHREAD_MUTEX_INITIALIZER;
static IncodexDeviceEnvironment *incodexDeviceEnvironments;
static void incodexCleanupDeviceEnvironment(void *raw);
static void incodexFinalizeDeviceFlight(napi_env env,void *raw,void *hint);
static void incodexReleaseDeviceEnvironment(IncodexDeviceEnvironment *environment) {
 bool destroy=false;
 pthread_mutex_lock(&environment->mutex);
 if(environment->references>0 && --environment->references==0)destroy=true;
 pthread_mutex_unlock(&environment->mutex);
 if(destroy){pthread_mutex_destroy(&environment->mutex);free(environment);}
}
static IncodexDeviceEnvironment *incodexDeviceEnvironmentFor(napi_env env) {
 pthread_mutex_lock(&incodexDeviceEnvironmentsMutex);
 for(IncodexDeviceEnvironment *item=incodexDeviceEnvironments;item;item=item->next){
  if(item->env==env){pthread_mutex_unlock(&incodexDeviceEnvironmentsMutex);return item;}
 }
 IncodexDeviceEnvironment *environment=calloc(1,sizeof(*environment));
 if(!environment || pthread_mutex_init(&environment->mutex,NULL)!=0){free(environment);pthread_mutex_unlock(&incodexDeviceEnvironmentsMutex);return NULL;}
 environment->env=env;environment->references=1;
 if(napi_add_env_cleanup_hook(env,incodexCleanupDeviceEnvironment,environment)!=napi_ok){
  pthread_mutex_destroy(&environment->mutex);free(environment);pthread_mutex_unlock(&incodexDeviceEnvironmentsMutex);return NULL;
 }
 environment->next=incodexDeviceEnvironments;incodexDeviceEnvironments=environment;
 pthread_mutex_unlock(&incodexDeviceEnvironmentsMutex);return environment;
}
static void incodexLinkDeviceFlightLocked(IncodexDeviceEnvironment *environment,IncodexDeviceFlight *flight) {
 flight->next=environment->flights;environment->flights=flight;flight->linked=true;environment->references++;
}
static void incodexUnlinkDeviceFlightLocked(IncodexDeviceEnvironment *environment,IncodexDeviceFlight *flight) {
 if(!flight->linked)return;
 IncodexDeviceFlight **cursor=&environment->flights;
 while(*cursor && *cursor!=flight)cursor=&(*cursor)->next;
 if(*cursor==flight)*cursor=flight->next;
 flight->next=NULL;flight->linked=false;
}
static void incodexFreeDeviceCompletion(IncodexDeviceCompletion *completion) {
 if(!completion)return;
 if(completion->output)CFRelease(completion->output);
 if(completion->keyId)CFRelease(completion->keyId);
 free(completion);
}
// --- N-API completion 与环境 cleanup 串行 join；worker 只负责退出。 ---
static void incodexJoinDeviceFlight(IncodexDeviceFlight *flight) {
 if(flight->threadStarted && !flight->threadJoined){pthread_join(flight->thread,NULL);flight->threadJoined=true;}
}
static void incodexDestroyDeviceFlight(IncodexDeviceFlight *flight) {
 IncodexDeviceEnvironment *environment=flight->environment;
 incodexFreeDeviceCompletion(flight->completion);
 if(flight->request)CFRelease(flight->request);if(flight->keyId)CFRelease(flight->keyId);
 for(int i=0;i<2;i++)if(flight->cancelPipe[i]>=0)close(flight->cancelPipe[i]);
 free(flight);
 if(environment)incodexReleaseDeviceEnvironment(environment);
}
static void incodexCancelDeviceFlight(IncodexDeviceFlight *flight) {
 char cancel=1;ssize_t sent;
 do{sent=write(flight->cancelPipe[1],&cancel,1);}while(sent<0 && errno==EINTR);
}
static void incodexRejectDevice(napi_env env,napi_deferred deferred,OSStatus status) {
 napi_value message,error,value;
 napi_create_string_utf8(env,"Cannot access the existing secure device key.",NAPI_AUTO_LENGTH,&message);
 napi_create_error(env,NULL,message,&error);
 const char *code=status==errSecItemNotFound ? "remote_control_device_key_unusable" : "remote_control_device_key_store_unavailable";
 napi_create_string_utf8(env,code,NAPI_AUTO_LENGTH,&value);napi_set_named_property(env,error,"errorCode",value);
 napi_create_int32(env,status,&value);napi_set_named_property(env,error,"osStatus",value);
 napi_reject_deferred(env,deferred,error);
}
static void incodexCompleteDevicePromise(napi_env env,napi_value callback,void *context,void *raw) {
 (void)callback;(void)context;IncodexDeviceCompletion *completion=raw;
 if(!env){incodexFreeDeviceCompletion(completion);return;}
 IncodexDeviceFlight *flight=completion->flight;
 if(flight)incodexJoinDeviceFlight(flight);
 if(flight && flight->environment){
  pthread_mutex_lock(&flight->environment->mutex);
  if(!flight->environment->closing)incodexUnlinkDeviceFlightLocked(flight->environment,flight);
  pthread_mutex_unlock(&flight->environment->mutex);
 }
 @autoreleasepool {
  id reply=completion->output ? [NSJSONSerialization JSONObjectWithData:(__bridge NSData *)completion->output options:0 error:NULL] : nil;
  if(![reply isKindOfClass:NSDictionary.class])reply=nil;
  id rawResult=reply[@"result"];
  NSDictionary *result=[rawResult isKindOfClass:NSDictionary.class] ? rawResult : nil;
  bool ok=!completion->failure && [reply[@"ok"] isEqual:@YES] &&
   [result isKindOfClass:NSDictionary.class] && [result[@"algorithm"] isEqual:@"ecdsa_p256_sha256"];
  NSArray *fields=completion->publicQuery ? @[@"keyId",@"algorithm",@"protectionClass",@"publicKeySpkiDerBase64"] : @[@"algorithm",@"signatureDerBase64"];
  if(ok && completion->publicQuery)ok=[result[@"keyId"] isEqual:(__bridge NSString *)completion->keyId] && [result[@"protectionClass"] isEqual:@"os_protected_nonextractable"];
  for(NSString *field in fields)if(![result[field] isKindOfClass:NSString.class] || [result[field] length]>16384)ok=false;
  if(ok){
   napi_value object;napi_create_object(env,&object);
   for(NSString *field in fields){napi_value value;napi_create_string_utf8(env,[result[field] UTF8String],NAPI_AUTO_LENGTH,&value);napi_set_named_property(env,object,field.UTF8String,value);}
   napi_resolve_deferred(env,completion->deferred,object);
  }else{
   id status=[reply isKindOfClass:NSDictionary.class] ? reply[@"status"] : nil;
   OSStatus error=[status isKindOfClass:NSNumber.class] ? [status intValue] : errSecNotAvailable;
   incodexRejectDevice(env,completion->deferred,error);
  }
 }
 incodexFreeDeviceCompletion(completion);
}
static void incodexFinalizeDeviceFlight(napi_env env,void *raw,void *hint) {
 (void)env;(void)hint;IncodexDeviceFlight *flight=raw;
 incodexJoinDeviceFlight(flight);
 incodexDestroyDeviceFlight(flight);
}
static void incodexCleanupDeviceEnvironment(void *raw) {
 IncodexDeviceEnvironment *environment=raw;
 pthread_mutex_lock(&incodexDeviceEnvironmentsMutex);
 IncodexDeviceEnvironment **cursor=&incodexDeviceEnvironments;
 while(*cursor && *cursor!=environment)cursor=&(*cursor)->next;
 if(*cursor==environment)*cursor=environment->next;
 pthread_mutex_unlock(&incodexDeviceEnvironmentsMutex);
 pthread_mutex_lock(&environment->mutex);
 environment->closing=true;
 IncodexDeviceFlight *flights=environment->flights;
 environment->flights=NULL;
 for(IncodexDeviceFlight *flight=flights;flight;flight=flight->next){
  flight->linked=false;
  if(flight->cancelPipe[1]>=0)incodexCancelDeviceFlight(flight);
 }
 pthread_mutex_unlock(&environment->mutex);
 for(IncodexDeviceFlight *flight=flights;flight;flight=flight->next){
  incodexJoinDeviceFlight(flight);
 }
 for(IncodexDeviceFlight *flight=flights;flight;){
  IncodexDeviceFlight *next=flight->next;
  if(!flight->threadsafeFinished){
   // --- join 后 cleanup 接管计数；abort 可同步触发 finalizer。 ---
   flight->threadsafeFinished=true;
   napi_release_threadsafe_function(flight->completionFunction,napi_tsfn_abort);
  }
  // --- 先保存 next，再不访问当前 Flight。 ---
  flight=next;
 }
 incodexReleaseDeviceEnvironment(environment);
}
static napi_value prepareDeviceKeyAccess(napi_env env,napi_callback_info info) {
 (void)info;bool ready=false;
 @autoreleasepool {
  if(deviceKeyBridgeHostReady()){
   NSDictionary *candidate=incodexDeviceHelperAtHome(NSHomeDirectory());
   @synchronized([NSDictionary class]) {
    if(candidate && (!preparedDeviceHelper || [preparedDeviceHelper isEqual:candidate])){if(!preparedDeviceHelper)preparedDeviceHelper=candidate;ready=true;}
   }
  }
 }
 napi_value result;napi_get_boolean(env,ready,&result);return result;
}
static void *incodexRunDeviceFlight(void *raw) {
 IncodexDeviceFlight *flight=raw;IncodexDeviceCompletion *completion=flight->completion;
 @autoreleasepool {
  if(incodexDeviceCancelPending(flight->cancelPipe[0]))completion->failure=7;
  else {
   NSDictionary *current=incodexDeviceHelperAtHome(NSHomeDirectory());
   NSDictionary *prepared=nil;@synchronized([NSDictionary class]){prepared=preparedDeviceHelper;}
   if(!current || ![current isEqual:prepared])completion->failure=1;
   else {
    NSData *data=incodexRunDeviceHelper(current,(__bridge NSData *)flight->request,
     INCODEX_DEVICE_HELPER_DEADLINE_SECONDS,!flight->publicQuery,flight->cancelPipe[0],&completion->failure);
    if(data)completion->output=CFBridgingRetain(data);
   }
  }
 }
 IncodexDeviceEnvironment *environment=flight->environment;
 pthread_mutex_lock(&environment->mutex);
 if(!environment->closing && !flight->threadsafeFinished){
  napi_status queued=napi_call_threadsafe_function(flight->completionFunction,completion,napi_tsfn_nonblocking);
  if(queued==napi_ok){
   flight->completion=NULL;
   flight->threadsafeFinished=true;
   napi_release_threadsafe_function(flight->completionFunction,napi_tsfn_release);
  }else if(queued==napi_closing){flight->threadsafeFinished=true;}
  else {
   flight->threadsafeFinished=true;
   napi_release_threadsafe_function(flight->completionFunction,napi_tsfn_abort);
  }
 }
 pthread_mutex_unlock(&environment->mutex);
 return NULL;
}
static napi_value dispatchDeviceKey(napi_env env,napi_callback_info info,bool publicQuery) {
 napi_deferred deferred;napi_value promise;napi_create_promise(env,&deferred,&promise);
 @autoreleasepool {
  bool prepared=false;@synchronized([NSDictionary class]){prepared=preparedDeviceHelper!=nil;}
  napi_value args[3];size_t argc=3;char key[128];size_t size=0;
  if(!deviceKeyBridgeHostReady() || !prepared || napi_get_cb_info(env,info,&argc,args,NULL,NULL)!=napi_ok ||
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
  IncodexDeviceEnvironment *environment=incodexDeviceEnvironmentFor(env);
  if(!environment){incodexRejectDevice(env,deferred,errSecNotAvailable);return promise;}
  IncodexDeviceFlight *flight=calloc(1,sizeof(*flight));
  IncodexDeviceCompletion *completion=calloc(1,sizeof(*completion));
  if(!flight || !completion){free(flight);free(completion);incodexRejectDevice(env,deferred,errSecAllocate);return promise;}
  flight->cancelPipe[0]=flight->cancelPipe[1]=-1;
  if(pipe(flight->cancelPipe)!=0){free(flight);free(completion);incodexRejectDevice(env,deferred,errSecNotAvailable);return promise;}
  for(int i=0;i<2;i++){
   fcntl(flight->cancelPipe[i],F_SETFD,FD_CLOEXEC);
   fcntl(flight->cancelPipe[i],F_SETFL,fcntl(flight->cancelPipe[i],F_GETFL)|O_NONBLOCK);
  }
  flight->environment=environment;flight->request=CFBridgingRetain(body);flight->keyId=CFBridgingRetain(keyId);flight->publicQuery=publicQuery;
  flight->completion=completion;completion->deferred=deferred;completion->keyId=CFBridgingRetain(keyId);
  completion->flight=flight;completion->publicQuery=publicQuery;
  pthread_mutex_lock(&environment->mutex);
  if(environment->closing){
   pthread_mutex_unlock(&environment->mutex);incodexFreeDeviceCompletion(completion);
   CFRelease(flight->request);CFRelease(flight->keyId);for(int i=0;i<2;i++)close(flight->cancelPipe[i]);free(flight);
   incodexRejectDevice(env,deferred,errSecNotAvailable);return promise;
  }
  incodexLinkDeviceFlightLocked(environment,flight);
  napi_value name;napi_create_string_utf8(env,"incodex-device-key",NAPI_AUTO_LENGTH,&name);
  napi_status created=napi_create_threadsafe_function(env,NULL,NULL,name,1,1,flight,incodexFinalizeDeviceFlight,NULL,
   incodexCompleteDevicePromise,&flight->completionFunction);
  if(created!=napi_ok){
   incodexUnlinkDeviceFlightLocked(environment,flight);pthread_mutex_unlock(&environment->mutex);
   incodexFinalizeDeviceFlight(NULL,flight,NULL);incodexRejectDevice(env,deferred,errSecNotAvailable);return promise;
  }
  napi_status unreferenced=napi_unref_threadsafe_function(env,flight->completionFunction);
  if(unreferenced!=napi_ok){
   incodexUnlinkDeviceFlightLocked(environment,flight);flight->threadsafeFinished=true;
   pthread_mutex_unlock(&environment->mutex);incodexRejectDevice(env,deferred,errSecNotAvailable);
   napi_release_threadsafe_function(flight->completionFunction,napi_tsfn_abort);return promise;
  }
  int started=pthread_create(&flight->thread,NULL,incodexRunDeviceFlight,flight);
  if(started!=0){
   incodexUnlinkDeviceFlightLocked(environment,flight);flight->threadsafeFinished=true;
   pthread_mutex_unlock(&environment->mutex);
   incodexRejectDevice(env,deferred,errSecNotAvailable);
   napi_release_threadsafe_function(flight->completionFunction,napi_tsfn_abort);
   return promise;
  }
  flight->threadStarted=true;
  pthread_mutex_unlock(&environment->mutex);
 }
 return promise;
}
static napi_value getDeviceKeyPublic(napi_env env,napi_callback_info info){return dispatchDeviceKey(env,info,true);}
static napi_value signDeviceKey(napi_env env,napi_callback_info info){return dispatchDeviceKey(env,info,false);}
#endif
