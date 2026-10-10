// Only the official OS-protected P-256 device-key request may use this fallback.
#import <Foundation/Foundation.h>
#import <Security/Security.h>

static bool incodexRemoteKeyEligible(CFDictionaryRef parameters) {
 if (!parameters || CFGetTypeID(parameters) != CFDictionaryGetTypeID()) return false;
 NSDictionary *p=(__bridge NSDictionary *)parameters;
 NSSet *top=[NSSet setWithArray:@[(__bridge id)kSecAttrKeyType,(__bridge id)kSecAttrKeySizeInBits,(__bridge id)kSecAttrLabel,(__bridge id)kSecPrivateKeyAttrs]];
 for(id name in p) if(![top containsObject:name]) return false;
 if (p[(__bridge id)kSecAttrTokenID] ||
     ![p[(__bridge id)kSecAttrKeyType] isEqual:(__bridge id)kSecAttrKeyTypeECSECPrimeRandom] ||
     ![p[(__bridge id)kSecAttrKeySizeInBits] isEqual:@256]) return false;
 id attrs=p[(__bridge id)kSecPrivateKeyAttrs];
 if (![attrs isKindOfClass:NSDictionary.class]) return false;
 NSSet *allowed=[NSSet setWithArray:@[(__bridge id)kSecAttrApplicationTag,(__bridge id)kSecAttrLabel,
   (__bridge id)kSecAttrIsPermanent,(__bridge id)kSecAttrIsExtractable,(__bridge id)kSecAttrAccessControl]];
 for(id name in attrs) if(![allowed containsObject:name]) return false;
 id tag=attrs[(__bridge id)kSecAttrApplicationTag];
 if (![tag isKindOfClass:NSData.class]) return false;
 NSString *name=[[NSString alloc] initWithData:tag encoding:NSUTF8StringEncoding];
 id access=attrs[(__bridge id)kSecAttrAccessControl];
 return [name hasPrefix:@"com.openai.codex.device-key."] &&
        name.length > @"com.openai.codex.device-key.".length &&
        [attrs[(__bridge id)kSecAttrIsPermanent] isEqual:@YES] &&
        [attrs[(__bridge id)kSecAttrIsExtractable] isEqual:@NO] &&
        access && CFGetTypeID((__bridge CFTypeRef)access)==SecAccessControlGetTypeID();
}

static CFDictionaryRef incodexRemoteKeyRetryAttributes(CFDictionaryRef parameters) CF_RETURNS_RETAINED;
static CFDictionaryRef incodexRemoteKeyRetryAttributes(CFDictionaryRef parameters) {
 if (!incodexRemoteKeyEligible(parameters)) return NULL;
 NSMutableDictionary *retry=[(__bridge NSDictionary *)parameters mutableCopy];
 retry[(__bridge id)kSecUseDataProtectionKeychain]=@NO;
 // A nested flag alone is ignored by the classic keychain for this API.
 retry[(__bridge id)kSecAttrIsExtractable]=@NO;
 return CFBridgingRetain(retry);
}
