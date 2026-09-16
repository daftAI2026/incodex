// @ts-nocheck
// objc-js returns undefined for C pointer return encodings. CF objects cross as id;
// CALayer KVC accepts those CF values, and retains them before we release ownership.
function createPermissionGraphics(objc) {
  const foundation = new objc.NobjcLibrary("/System/Library/Frameworks/Foundation.framework/Foundation");
  new objc.NobjcLibrary("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics");
  function store(layer, key, create, signature, args, release) {
    const value = objc.callFunction(create, { returns: "@", args: signature }, ...args);
    if (!value) throw new Error(`Native permission ${key} could not be created`);
    try { layer.setValue$forKey$(value, foundation.NSString.stringWithUTF8String$(key)); }
    finally { objc.callFunction(release, { returns: "v", args: ["@"] }, value); }
  }
  return {
    setColor(layer, key, components) {
      store(layer, key, "CGColorCreateGenericRGB", ["d", "d", "d", "d"], components, "CGColorRelease");
    },
    setBlackColor(layer, key, alpha) {
      store(layer, key, "CGColorCreateGenericRGB", ["d", "d", "d", "d"], [0, 0, 0, alpha], "CGColorRelease");
    },
    setRoundedShadowPath(layer, bounds, radius) {
      store(layer, "shadowPath", "CGPathCreateWithRoundedRect", ["{CGRect={CGPoint=dd}{CGSize=dd}}", "d", "d", "^v"], [bounds, radius, radius, null], "CGPathRelease");
    },
  };
}
export { createPermissionGraphics };
