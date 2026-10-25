/**
 * [INPUT]: 依赖测试专用 parent-exit watcher 与 Darwin PROC 事件。
 * [OUTPUT]: 验证宿主退出能终止短命消费者，且启动前不触发 Keychain。
 * [POS]: 进程生命周期回归；用 posix_spawn 分层创建监督者与观察者。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#import <Foundation/Foundation.h>
#import <dispatch/dispatch.h>
#import <errno.h>
#import <limits.h>
#import <libproc.h>
#import <signal.h>
#import <spawn.h>
#import <stdio.h>
#import <stdlib.h>
#import <string.h>
#import <sys/proc_info.h>
#import <sys/wait.h>
#import <unistd.h>
#import "device-key-consumer.h"

extern char **environ;

static int spawnWatcher(const char *executable, pid_t *watcherPID) {
 int readyPipe[2]={-1,-1};
 if(pipe(readyPipe)!=0) return errno;
 posix_spawn_file_actions_t actions;
 if(posix_spawn_file_actions_init(&actions)!=0) {close(readyPipe[0]);close(readyPipe[1]);return EIO;}
 posix_spawn_file_actions_adddup2(&actions,readyPipe[1],STDOUT_FILENO);
 posix_spawn_file_actions_addclose(&actions,readyPipe[0]);
 posix_spawn_file_actions_addclose(&actions,readyPipe[1]);
 char *arguments[]={(char *)executable,"--watcher",NULL};
 int result=posix_spawn(watcherPID,executable,&actions,NULL,arguments,environ);
 posix_spawn_file_actions_destroy(&actions);
 close(readyPipe[1]);
 if(result!=0) {close(readyPipe[0]);return result;}
 char outcome='F';ssize_t count;
 do {count=read(readyPipe[0],&outcome,1);} while(count<0 && errno==EINTR);
 close(readyPipe[0]);
 if(count!=1) outcome='F';
 dprintf(STDOUT_FILENO,"%d %c\n",*watcherPID,outcome);
 return outcome=='R' ? 0 : EPROTO;
}

static int watcherMode(void) {
 pid_t parentPID=getppid();
 struct proc_bsdinfo parent={0};
 char path[PROC_PIDPATHINFO_MAXSIZE]={0};
 if(parentPID<=1 || proc_pidinfo(parentPID,PROC_PIDTBSDINFO,0,&parent,sizeof(parent))!=sizeof(parent) ||
    parent.pbi_pid!=(uint32_t)parentPID || proc_pidpath(parentPID,path,sizeof(path))<=0) {
  write(STDOUT_FILENO,"F",1);return 71;
 }
 __attribute__((objc_precise_lifetime)) dispatch_source_t source=
   incodexDeviceKeyTestWatchParentExit(parentPID,&parent,path);
 if(!source) {write(STDOUT_FILENO,"F",1);return 71;}
 write(STDOUT_FILENO,"R",1);
 for(;;) pause();
}

static int coordinatorMode(const char *executable) {
 pid_t watcherPID=-1;
 int result=spawnWatcher(executable,&watcherPID);
 if(result!=0) return 72;
 char acknowledgment=0;ssize_t count;
 do {count=read(STDIN_FILENO,&acknowledgment,1);} while(count<0 && errno==EINTR);
 return count==1 && acknowledgment=='X' ? 0 : 73;
}

static bool readLine(int descriptor, char *buffer, size_t capacity) {
 size_t length=0;
 while(length+1<capacity) {
  char character;ssize_t count=read(descriptor,&character,1);
  if(count<0 && errno==EINTR) continue;
  if(count!=1) return false;
  if(character=='\n') {buffer[length]=0;return true;}
  buffer[length++]=character;
 }
 return false;
}

static void signalWhenProcessExits(dispatch_source_t source, dispatch_semaphore_t semaphore) {
 dispatch_source_set_event_handler(source,^{dispatch_semaphore_signal(semaphore);});
}

int main(int argc, char **argv) {
 if(argc==2 && strcmp(argv[1],"--watcher")==0) return watcherMode();
 if(argc==2 && strcmp(argv[1],"--coordinator")==0) return coordinatorMode(argv[0]);

 char executable[PATH_MAX]={0};
 if(!realpath(argv[0],executable)) {perror("realpath");return 1;}
 int statusPipe[2]={-1,-1},ackPipe[2]={-1,-1};
 if(pipe(statusPipe)!=0 || pipe(ackPipe)!=0) {perror("pipe");return 1;}
 posix_spawn_file_actions_t actions;
 if(posix_spawn_file_actions_init(&actions)!=0) {perror("spawn actions");return 1;}
 posix_spawn_file_actions_adddup2(&actions,statusPipe[1],STDOUT_FILENO);
 posix_spawn_file_actions_adddup2(&actions,ackPipe[0],STDIN_FILENO);
 posix_spawn_file_actions_addclose(&actions,statusPipe[0]);
 posix_spawn_file_actions_addclose(&actions,statusPipe[1]);
 posix_spawn_file_actions_addclose(&actions,ackPipe[0]);
 posix_spawn_file_actions_addclose(&actions,ackPipe[1]);
 char *arguments[]={executable,"--coordinator",NULL};
 pid_t coordinatorPID=-1;
 int spawned=posix_spawn(&coordinatorPID,executable,&actions,NULL,arguments,environ);
 posix_spawn_file_actions_destroy(&actions);
 close(statusPipe[1]);statusPipe[1]=-1;close(ackPipe[0]);ackPipe[0]=-1;
 if(spawned!=0) {fprintf(stderr,"spawn coordinator: %d\n",spawned);return 1;}

 int exitStatus=0;pid_t watcherPID=-1;dispatch_source_t exitSource=NULL;
 dispatch_semaphore_t exited=dispatch_semaphore_create(0);
 char line[64]={0},outcome='F';int parsedPID=0;
 if(!readLine(statusPipe[0],line,sizeof(line)) || sscanf(line,"%d %c",&parsedPID,&outcome)!=2 || parsedPID<=1) {
  fprintf(stderr,"lifecycle test did not receive watcher readiness\n");exitStatus=1;goto cleanup;
 }
 watcherPID=(pid_t)parsedPID;
 if(outcome!='R') {
  fprintf(stderr,"lifecycle watcher failed before readiness\n");exitStatus=1;goto cleanup;
 }
 exitSource=dispatch_source_create(DISPATCH_SOURCE_TYPE_PROC,(uintptr_t)watcherPID,
   DISPATCH_PROC_EXIT,dispatch_get_global_queue(QOS_CLASS_DEFAULT,0));
 if(!exitSource) {fprintf(stderr,"cannot observe watcher exit\n");exitStatus=1;goto cleanup;}
 signalWhenProcessExits(exitSource,exited);
 dispatch_resume(exitSource);

 if(write(ackPipe[1],"X",1)!=1) {fprintf(stderr,"cannot release coordinator\n");exitStatus=1;goto cleanup;}
 close(ackPipe[1]);ackPipe[1]=-1;
 int coordinatorStatus=0;
 while(waitpid(coordinatorPID,&coordinatorStatus,0)<0 && errno==EINTR) {}
 coordinatorPID=-1;
 if(!WIFEXITED(coordinatorStatus) || WEXITSTATUS(coordinatorStatus)!=0) {
  fprintf(stderr,"coordinator did not exit cleanly\n");exitStatus=1;goto cleanup;
 }
 if(dispatch_semaphore_wait(exited,dispatch_time(DISPATCH_TIME_NOW,NSEC_PER_SEC))!=0) {
  fprintf(stderr,"watcher remained alive after its parent exited\n");exitStatus=1;goto cleanup;
 }
 puts("device-key helper parent-exit lifecycle passed");

cleanup:
 if(statusPipe[0]>=0) close(statusPipe[0]);
 if(ackPipe[1]>=0) close(ackPipe[1]);
 if(coordinatorPID>0) {kill(coordinatorPID,SIGKILL);waitpid(coordinatorPID,NULL,0);}
 if(watcherPID>0 && exitStatus!=0) kill(watcherPID,SIGKILL);
 if(exitSource) dispatch_source_cancel(exitSource);
 return exitStatus;
}
