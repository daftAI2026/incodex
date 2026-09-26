// Objective-C calling conventions only; UI and animation policy are untouched.
const rawValues=new WeakMap();
const declaredReturns=new Map();
ObjC.bindFunction('dlopen',['void *',['string','int']]);
ObjC.bindFunction('IncodexPermissionObjectFromPointer',['id',['void *']]);
function unwrap(value){if(value===null)return $();return ((typeof value==='object'||typeof value==='function')&&value!==null&&rawValues.has(value))?rawValues.get(value):value;}
function jxaName(selector){return selector.replace(/:([a-z])/g,(_,c)=>c.toUpperCase()).replace(/:/g,'');}
function wrap(raw){
  if(typeof raw!=='function')return raw;
  // CoreFoundation opaque references are callable JXA values, not ObjC wrappers.
  if(typeof raw.isNil!=='function')return raw;
  if(raw.isNil())return null;
  const cache=new Map();
  const result=new Proxy({}, {get(_,key){
    if(key==='then')return undefined;
    if(key===Symbol.toPrimitive)return ()=>String(ObjC.unwrap(raw.description));
    if(key==='toString')return ()=>String(ObjC.unwrap(raw.description));
    if(typeof key!=='string')return undefined;
    if(cache.has(key))return cache.get(key);
    const selector=key.replace(/\$/g,':');
    if(!raw.respondsToSelector(selector))return undefined;
    const signature=raw.methodSignatureForSelector(selector);
    // Retain the caller's explicit ABI contract when JXA infers an inherited
    // Intel BOOL as char. Do not globally coerce all signed chars to boolean.
    const className=String(ObjC.unwrap(raw.class.description));
    const returnType=declaredReturns.get(className)?.get(selector)||String(signature.methodReturnType);
    const method=(...args)=>{
      if(selector==='startForWindow:handler:'||selector==='startForScreen:handler:'){
        if(!args[1]?.nativeTarget)throw Error('Invalid display callback');
        const fn=selector==='startForWindow:handler:'?'IncodexPermissionStartWindowClock':'IncodexPermissionStartScreenClock';
        return $[fn](raw,unwrap(args[0]),args[1].nativeTarget);
      }
      const mapped=jxaName(selector);
      const value=selector.includes(':')?raw[mapped](...args.map(unwrap)):raw[mapped];
      if(['q','Q','l','L','i','I','s','S','c','C','f','d'].includes(returnType))return Number(value);
      if(returnType==='B')return Boolean(value);
      return wrap(value);
    };
    cache.set(key,method);return method;
  }});
  rawValues.set(result,raw);return result;
}
const encodingNames={v:'void',B:'bool','@':'id',':':'selector',d:'double',q:'long long',Q:'unsigned long long'};
function methodTypes(encoding){
  const tokens=encoding.match(/\{[^}]+\}|[vB@:dqQ]/g);
  if(!tokens||tokens.join('')!==encoding||tokens[1]!=='@'||tokens[2]!==':')throw Error('unsupported method encoding '+encoding);
  const name=token=>token==='{CGPoint=dd}'?'CGPoint':encodingNames[token];
  return [name(tokens[0]),tokens.slice(3).map(name)];
}
let blockSequence=0;
ObjC.bindFunction('IncodexPermissionStartWindowClock',['void',['id','id','id']]);
ObjC.bindFunction('IncodexPermissionStartScreenClock',['void',['id','id','id']]);
const objc={
  NobjcLibrary:class {
    constructor(path){
      if(path.endsWith('.dylib')){
        if(!$.dlopen(path,2))throw Error('Cannot load verified permission library');
      }else{
        const match=path.match(/\/([^/]+)\.framework\//);
        if(!match)throw Error('unsupported library '+path);
        ObjC.import(match[1]);
      }
      return new Proxy({}, {get(_,key){if(typeof key!=='string')return undefined;return wrap($.NSClassFromString(key));}});
    }
  },
  NobjcClass:{define(spec){
    const methods={};
    declaredReturns.set(spec.name,new Map(Object.entries(spec.methods).map(([selector,entry])=>[selector,entry.types[0]])));
    for(const [selector,entry]of Object.entries(spec.methods)){
      const implementation=function(...args){return unwrap(entry.implementation(wrap(this),...args.map(wrap)));};
      // For existing protocol/superclass methods, use Apple's own ABI metadata.
      const inherited=$.NSClassFromString(spec.superclass).instancesRespondToSelector(selector);
      methods[selector]=(spec.protocols||inherited)?implementation:{types:methodTypes(entry.types),implementation};
    }
    ObjC.registerSubclass({name:spec.name,superclass:spec.superclass,methods,protocols:spec.protocols||[]});
    return wrap($.NSClassFromString(spec.name));
  }},
  typedBlock(signature,callback){
    if(signature.returns!=='v'||signature.args.join('')!=='ddd')throw Error('unsupported block signature');
    const name='IncodexHostTick_'+(++blockSequence);
    ObjC.registerSubclass({name,superclass:'NSObject',methods:{'tick:duration:target:':{types:['void',['double','double','double']],implementation:(a,b,c)=>callback(a,b,c)}}});
    return {nativeTarget:$.NSClassFromString(name).alloc.init};
  },
  callFunction(name,signature,...args){
    const allowed=['CGWindowListCopyWindowInfo','CFRelease','CGColorCreateGenericRGB','CGColorRelease','CGPathCreateMutable','CGPathCreateWithRoundedRect','CGPathMoveToPoint','CGPathAddLineToPoint','CGPathAddCurveToPoint','CGPathCloseSubpath','CGPathAddRect','CGPathAddPath','CGPathRelease'];
    if(!allowed.includes(name))throw Error('unsupported C function '+name);
    const value=$[name](...args.map(unwrap));
    return wrap(signature.returns==='@'?$.IncodexPermissionObjectFromPointer(value):value);
  }
};
