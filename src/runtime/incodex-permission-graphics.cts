// @ts-nocheck
// objc-js returns undefined for C pointer return encodings. CF objects cross as id;
// CALayer KVC accepts those CF values, and retains them before we release ownership.
// Exported by macOS 27 SwiftUI RoundedRectangle(cornerRadius:24, style:.continuous)
// for this fixed 518x80 permission row. Coordinates preserve the system curve.
const PLACEHOLDER_PATH = [[0,518,40],[1,518,43.31204128265381],[3,518,53.87623977661133,518,59.15823173522949,516.2021263837814,64.84414434432983],[3,513.942559838295,71.05222368240356,509.05222368240356,75.94255983829498,502.84414434432983,78.20212638378143],[3,497.1582317352295,80,491.8762397766113,80,481.3120412826538,80],[1,36.68795871734619,80],[3,26.123760223388672,80,20.841768264770508,80,15.155855655670166,78.20212638378143],[3,8.947776317596436,75.94255983829498,4.057440161705017,71.05222368240356,1.797873616218567,64.84414434432983],[3,0,59.15823173522949,0,53.87623977661133,0,43.31204128265381],[1,0,36.68795871734619],[3,0,26.123760223388672,0,20.841768264770508,1.797873616218567,15.155855655670166],[3,4.057440161705017,8.947776317596436,8.947776317596436,4.057440161705017,15.155855655670166,1.797873616218567],[3,20.841768264770508,0,26.123760223388672,0,36.68795871734619,0],[1,481.3120412826538,0],[3,491.8762397766113,0,497.1582317352295,0,502.84414434432983,1.797873616218567],[3,509.05222368240356,4.057440161705017,513.942559838295,8.947776317596436,516.2021263837814,15.155855655670166],[3,518,20.841768264770508,518,26.123760223388672,518,36.68795871734619],[4]];
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
    setPlaceholderPath(layer) {
      const path = objc.callFunction("CGPathCreateMutable", { returns: "@", args: [] });
      if (!path) throw new Error("Native permission path could not be created");
      try {
        for (const [kind, ...points] of PLACEHOLDER_PATH) {
          const name = kind === 0 ? "CGPathMoveToPoint" : kind === 1 ? "CGPathAddLineToPoint" : kind === 3 ? "CGPathAddCurveToPoint" : "CGPathCloseSubpath";
          if (kind === 4) objc.callFunction(name, { returns: "v", args: ["@"] }, path);
          else objc.callFunction(name, { returns: "v", args: ["@", "^v", ...points.map(() => "d")] }, path, null, ...points);
        }
        layer.setValue$forKey$(path, foundation.NSString.stringWithUTF8String$("path"));
      } finally { objc.callFunction("CGPathRelease", { returns: "v", args: ["@"] }, path); }
    },
    setRoundedShadowPath(layer, bounds, radius) {
      store(layer, "shadowPath", "CGPathCreateWithRoundedRect", ["{CGRect={CGPoint=dd}{CGSize=dd}}", "d", "d", "^v"], [bounds, radius, radius, null], "CGPathRelease");
    },
  };
}
export { createPermissionGraphics };
