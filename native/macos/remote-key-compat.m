#import "remote-key-policy.h"
#import "vendor/fishhook/fishhook.h"
#import "vendor/node/node_api.h"
#import <mach-o/dyld.h>
#import <dlfcn.h>

static const struct mach_header *installedImage;
static void *originalImport;

static SecKeyRef compatibleCreate(CFDictionaryRef parameters, CFErrorRef *error) {
 CFErrorRef initial=NULL;
 SecKeyRef key=SecKeyCreateRandomKey(parameters,&initial);
 Dl_info caller={0};
 bool scoped=dladdr(__builtin_return_address(0),&caller) && caller.dli_fbase==installedImage;
 if (key || !scoped || !initial || CFErrorGetCode(initial)!=errSecMissingEntitlement ||
     !CFEqual(CFErrorGetDomain(initial),kCFErrorDomainOSStatus) || !incodexRemoteKeyEligible(parameters)) {
  if (error) *error=initial; else if(initial) CFRelease(initial);
  return key;
 }
 CFRelease(initial);
 initial=NULL;
 CFDictionaryRef retry=incodexRemoteKeyRetryAttributes(parameters);
 key=SecKeyCreateRandomKey(retry,&initial);
 CFRelease(retry);
 if (key) {
  CFDictionaryRef attrs=SecKeyCopyAttributes(key);
  bool safe=attrs && CFEqual(CFDictionaryGetValue(attrs,kSecAttrIsExtractable) ?: kCFNull,kCFBooleanFalse);
  if(attrs) CFRelease(attrs);
  if(!safe) {
   // Delete only the newly returned object, never by a broad application tag.
   NSDictionary *query=@{(__bridge id)kSecClass:(__bridge id)kSecClassKey,
                         (__bridge id)kSecValueRef:(__bridge id)key};
   SecItemDelete((__bridge CFDictionaryRef)query);
   CFRelease(key); key=NULL;
   if(initial) CFRelease(initial);
   initial=CFErrorCreate(NULL,kCFErrorDomainOSStatus,errSecNotAvailable,NULL);
  }
 }
 if(error) *error=initial; else if(initial) CFRelease(initial);
 return key;
}

static bool trustedModule(NSString *path) {
 NSString *expected=[NSBundle.mainBundle.resourcePath stringByAppendingPathComponent:@"native/remote-control-device-key.node"];
 if(![path isEqualToString:expected] || ![path isEqualToString:path.stringByResolvingSymlinksInPath]) return false;
 SecCodeRef self=NULL;CFDictionaryRef info=NULL;
 bool adhoc=false;
 if(SecCodeCopySelf(kSecCSDefaultFlags,&self)==errSecSuccess &&
    SecCodeCopySigningInformation(self,kSecCSSigningInformation,&info)==errSecSuccess) {
  NSDictionary *d=(__bridge NSDictionary *)info;
  adhoc=[d[(__bridge id)kSecCodeInfoIdentifier] isEqual:@"com.openai.codex"] &&
        ([d[(__bridge id)kSecCodeInfoFlags] unsignedIntValue] & kSecCodeSignatureAdhoc);
 }
 if(info) CFRelease(info);if(self) CFRelease(self);
 if(!adhoc) return false;
 SecStaticCodeRef module=NULL;SecRequirementRef requirement=NULL;
 NSString *rule=@"anchor apple generic and certificate leaf[subject.OU] = \"2DC432GLL2\"";
 bool valid=SecStaticCodeCreateWithPath((__bridge CFURLRef)[NSURL fileURLWithPath:path],kSecCSDefaultFlags,&module)==errSecSuccess &&
   SecRequirementCreateWithString((__bridge CFStringRef)rule,kSecCSDefaultFlags,&requirement)==errSecSuccess &&
   SecStaticCodeCheckValidity(module,kSecCSStrictValidate,requirement)==errSecSuccess;
 if(requirement) CFRelease(requirement);if(module) CFRelease(module);
 return valid;
}

static napi_value install(napi_env env,napi_callback_info info) {
 size_t argc=1; napi_value argument;char path[4096];size_t length=0;bool success=false;
 if(napi_get_cb_info(env,info,&argc,&argument,NULL,NULL)==napi_ok && argc==1 &&
    napi_get_value_string_utf8(env,argument,path,sizeof(path),&length)==napi_ok && length>0 && length<sizeof(path)-1) {
  @autoreleasepool {
   NSString *requested=[NSString stringWithUTF8String:path];
   if(trustedModule(requested)) {
    char canonical[PATH_MAX];
    if(!realpath(path,canonical)) { napi_value no;napi_get_boolean(env,false,&no);return no; }
    for(uint32_t i=0;i<_dyld_image_count();i++) {
     const char *name=_dyld_get_image_name(i);
     if(name && strcmp(name,canonical)==0) {
      const struct mach_header *image=_dyld_get_image_header(i);
      if(installedImage) { success=installedImage==image;break; }
      installedImage=image;
      struct rebinding binding={"SecKeyCreateRandomKey",compatibleCreate,&originalImport};
      success=rebind_symbols_image((void *)image,_dyld_get_image_vmaddr_slide(i),&binding,1)==0 && originalImport!=NULL;
      if(!success) installedImage=NULL;
      break;
     }
    }
   }
  }
 }
 napi_value result;napi_get_boolean(env,success,&result);return result;
}
static napi_value initialize(napi_env env,napi_value exports) {
 napi_value function;napi_create_function(env,"install",NAPI_AUTO_LENGTH,install,NULL,&function);
 napi_set_named_property(env,exports,"install",function);return exports;
}
NAPI_MODULE(NODE_GYP_MODULE_NAME,initialize)
