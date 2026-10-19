/**
 * [INPUT]: 依赖 Security 宿主签名与既有公开证书登记。
 * [OUTPUT]: 返回 Unsupported、Adhoc 或 RegisteredLocal 身份。
 * [POS]: 原生兼容模块的只读身份边界；不代表任何系统权限已授予。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
#pragma once
#import <Foundation/Foundation.h>
#import <Security/Security.h>
#import <CommonCrypto/CommonDigest.h>
#import <errno.h>
#import <fcntl.h>
#import <sys/stat.h>
#import <unistd.h>

typedef NS_ENUM(NSUInteger, IncodexHostIdentity) {
 IncodexHostIdentityUnsupported,
 IncodexHostIdentityAdhoc,
 IncodexHostIdentityRegisteredLocal,
};

// --- 只读登记：目录 FD 固定路径，拒绝重定向和非私有资产。 ---
static int incodexOpenSigningDirectory(void) {
 int descriptor=open(NSHomeDirectory().fileSystemRepresentation,O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
 struct stat metadata;
 if(descriptor<0) return -1;
 if(fstat(descriptor,&metadata)!=0 || metadata.st_uid!=geteuid() || (metadata.st_mode&0022)) {
  close(descriptor);return -1;
 }
 for(NSString *name in @[@".incodex",@"macos-signing"]) {
  int child=openat(descriptor,name.fileSystemRepresentation,O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC);
  close(descriptor);descriptor=child;
  if(descriptor<0) return -1;
  if(fstat(descriptor,&metadata)!=0 || metadata.st_uid!=geteuid() || (metadata.st_mode&0777)!=0700) {
   close(descriptor);return -1;
  }
 }
 return descriptor;
}

static NSData *incodexReadSigningFile(int directory,const char *name,size_t maximum) {
 int descriptor=openat(directory,name,O_RDONLY|O_NOFOLLOW|O_CLOEXEC|O_NONBLOCK);
 if(descriptor<0) return nil;
 struct stat before,after;
 NSData *result=nil;
 if(fstat(descriptor,&before)==0 && S_ISREG(before.st_mode) && before.st_uid==geteuid() &&
    (before.st_mode&0777)==0600 && before.st_nlink==1 && before.st_size>0 && (uint64_t)before.st_size<=maximum) {
  NSMutableData *bytes=[NSMutableData dataWithLength:(NSUInteger)before.st_size];
  size_t offset=0;
  while(offset<bytes.length) {
   ssize_t count=read(descriptor,(char *)bytes.mutableBytes+offset,bytes.length-offset);
   if(count<0 && errno==EINTR) continue;
   if(count<=0) break;
   offset+=(size_t)count;
  }
  char extra;ssize_t tail;
  do { tail=read(descriptor,&extra,1); } while(tail<0 && errno==EINTR);
  if(offset==bytes.length && tail==0 && fstat(descriptor,&after)==0 &&
     before.st_dev==after.st_dev && before.st_ino==after.st_ino && before.st_size==after.st_size &&
     before.st_uid==after.st_uid && before.st_mode==after.st_mode && after.st_nlink==1 &&
     before.st_mtimespec.tv_sec==after.st_mtimespec.tv_sec && before.st_mtimespec.tv_nsec==after.st_mtimespec.tv_nsec) result=bytes;
 }
 close(descriptor);return result;
}

static NSString *incodexCertificateDigest(NSData *certificate,bool sha256) {
 unsigned char bytes[CC_SHA256_DIGEST_LENGTH];
 if(sha256) CC_SHA256(certificate.bytes,(CC_LONG)certificate.length,bytes);
 else CC_SHA1(certificate.bytes,(CC_LONG)certificate.length,bytes);
 NSMutableString *text=[NSMutableString string];
 for(size_t i=0;i<(sha256 ? CC_SHA256_DIGEST_LENGTH : CC_SHA1_DIGEST_LENGTH);i++) [text appendFormat:@"%02x",bytes[i]];
 return text;
}

static NSData *incodexRegisteredSigningCertificate(void) {
 int directory=incodexOpenSigningDirectory();
 if(directory<0) return nil;
 NSData *registration=incodexReadSigningFile(directory,"identity.json",4096);
 NSData *certificate=incodexReadSigningFile(directory,"certificate.der",64*1024);
 close(directory);
 if(!registration || !certificate) return nil;
 id metadata=[NSJSONSerialization JSONObjectWithData:registration options:0 error:NULL];
 if(![metadata isKindOfClass:NSDictionary.class]) return nil;
 NSSet *fields=[NSSet setWithArray:@[@"schemaVersion",@"keychainPath",@"certificateSha1",@"certificateSha256",@"storePath",@"storeSha256"]];
 id schema=metadata[@"schemaVersion"];
 id storeHash=metadata[@"storeSha256"];
 NSCharacterSet *nonHex=[[NSCharacterSet characterSetWithCharactersInString:@"0123456789abcdef"] invertedSet];
 if(![storeHash isKindOfClass:NSString.class] || [storeHash length]!=64 || [storeHash rangeOfCharacterFromSet:nonHex].location!=NSNotFound) return nil;
 NSString *storePath=[NSHomeDirectory() stringByAppendingPathComponent:[@".incodex/helpers/macos-signing/" stringByAppendingFormat:@"%@/incodex-signing-store",storeHash]];
 if(![storePath isEqual:metadata[@"storePath"]]) return nil;
 if(![fields isEqualToSet:[NSSet setWithArray:[metadata allKeys]]] ||
    ![schema isKindOfClass:NSNumber.class] || CFGetTypeID((__bridge CFTypeRef)schema)==CFBooleanGetTypeID() || ![schema isEqual:@1] ||
    ![[NSHomeDirectory() stringByAppendingPathComponent:@".incodex/macos-signing/identity.keychain-db"] isEqual:metadata[@"keychainPath"]] ||
    ![incodexCertificateDigest(certificate,false) isEqual:metadata[@"certificateSha1"]] ||
    ![incodexCertificateDigest(certificate,true) isEqual:metadata[@"certificateSha256"]]) return nil;
 return certificate;
}

// --- 身份不是授权：只认现有 ad-hoc 或登记证书，不读取私钥。 ---
static IncodexHostIdentity incodexHostIdentity(void) {
 SecCodeRef self=NULL;CFDictionaryRef information=NULL;
 IncodexHostIdentity identity=IncodexHostIdentityUnsupported;
 if(SecCodeCopySelf(kSecCSDefaultFlags,&self)==errSecSuccess &&
    SecCodeCopySigningInformation(self,kSecCSSigningInformation,&information)==errSecSuccess) {
  NSDictionary *info=(__bridge NSDictionary *)information;
  if([info[(__bridge id)kSecCodeInfoIdentifier] isEqual:@"com.openai.codex"]) {
   if([info[(__bridge id)kSecCodeInfoFlags] unsignedIntValue]&kSecCodeSignatureAdhoc) identity=IncodexHostIdentityAdhoc;
   else {
    id team=info[(__bridge id)kSecCodeInfoTeamIdentifier];
    id certificates=info[(__bridge id)kSecCodeInfoCertificates];
    NSData *registered=incodexRegisteredSigningCertificate();
    if((!team || [team isEqual:@""]) && registered && [certificates isKindOfClass:NSArray.class] && [certificates count]>0) {
     CFTypeRef leafCertificate=(__bridge CFTypeRef)[certificates firstObject];
     if(CFGetTypeID(leafCertificate)==SecCertificateGetTypeID()) {
      NSData *leaf=CFBridgingRelease(SecCertificateCopyData((SecCertificateRef)leafCertificate));
      if([registered isEqual:leaf] && SecCodeCheckValidity(self,kSecCSStrictValidate,NULL)==errSecSuccess) identity=IncodexHostIdentityRegisteredLocal;
     }
    }
   }
  }
 }
 if(information) CFRelease(information);if(self) CFRelease(self);
 return identity;
}
