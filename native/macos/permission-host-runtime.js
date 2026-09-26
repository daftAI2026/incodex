// Thin host environment. Build substitutes the exact existing UI/locator
// bundles below; it does not translate their layout or animation code.
ObjC.import('AppKit');
ObjC.import('CoreGraphics');
ObjC.import('QuartzCore');
const bridge = $.NSClassFromString('IncodexPermissionHostBridge');
const nativeString = value => String(ObjC.unwrap(value));
const process = { platform: 'darwin', pid: Number($.NSProcessInfo.processInfo.processIdentifier), getuid: () => Number(ObjC.unwrap(bridge.userID)) };
const performance = { now: () => Number($.NSProcessInfo.processInfo.systemUptime) * 1000 };
const executable = nativeString($.NSBundle.mainBundle.executablePath);
const __dirname = nativeString($(executable).stringByDeletingLastPathComponent);
function normalizePath(value){const parts=[];for(const part of String(value).split('/')){if(!part||part==='.')continue;if(part==='..')parts.pop();else parts.push(part);}return '/'+parts.join('/');}
const path = {
  isAbsolute: value => String(value).startsWith('/'),
  resolve: value => {if(!String(value).startsWith('/'))throw Error('Absolute asset path required');return normalizePath(value);},
  dirname: value => {const parts=normalizePath(value).split('/');parts.pop();return parts.join('/')||'/';},
  join: (...parts) => normalizePath(parts.join('/')),
};
const fs = {
  lstatSync(file) {
    const info = bridge.fileInfo(String(file));
    if (info.isNil()) throw Error('Cannot inspect permission asset');
    const value = ObjC.deepUnwrap(info);
    return {size:Number(value.size),uid:Number(value.uid),mode:Number(value.mode),
      isFile:()=>Boolean(value.file),isDirectory:()=>Boolean(value.directory),isSymbolicLink:()=>Boolean(value.symlink)};
  },
  readFileSync(file) {
    const data=bridge.readData(String(file));
    if(data.isNil())throw Error('Cannot read permission asset');
    return {data,toString:()=>nativeString($.NSString.alloc.initWithDataEncoding(data,4))};
  },
};
const crypto={createHash(algorithm){if(algorithm!=='sha256')throw Error('Unsupported hash');let data=null;return {update(bytes){data=bytes.data;return this;},digest(format){if(format!=='hex'||!data)throw Error('Invalid hash request');return nativeString(bridge.sha256(data));}};}};
function require(name){
  if(name==='node:fs')return fs;
  if(name==='node:path')return path;
  if(name==='node:crypto')return crypto;
  if(name==='node:url')return {pathToFileURL:()=>{throw Error('Unexpected dynamic module import');}};
  throw Error('Unsupported permission host import: '+name);
}

// __INCODEX_OBJC_ADAPTER__

let nextTimer=0, guide=null, configuration=null, creating=false, closed=false;
const timers=new Map(), events=[];
function emit(type,message){if(events.length>=32)throw Error('Permission host event overflow');events.push({type,...(message?{message:String(message)}:{})});}
function timer(callback,ms,repeat){
  const id=++nextTimer;
  const native=$.NSTimer.timerWithTimeIntervalRepeatsBlock(Math.max(Number(ms)/1000,.001),repeat,()=>{
    if(!repeat)timers.delete(id);
    try{callback();}catch(error){emit('error',String(error));}
  });
  timers.set(id,native);$.NSRunLoop.mainRunLoop.addTimerForMode(native,$.NSRunLoopCommonModes);return id;
}
function setTimeout(callback,ms){return timer(callback,ms,false);}
function setInterval(callback,ms){return timer(callback,ms,true);}
function clearTimeout(id){const native=timers.get(id);if(native){native.invalidate;timers.delete(id);}}
function clearInterval(id){clearTimeout(id);}
function loadOriginal(source){const module={exports:{}};new Function('module','exports','require','__dirname',source)(module,module.exports,require,__dirname);return module.exports;}
const ui=loadOriginal(__INCODEX_ORIGINAL_UI_JSON__);
const locatorModule=loadOriginal(__INCODEX_ORIGINAL_LOCATOR_JSON__);
function configure(json){if(configuration||closed)throw Error('Already configured');configuration=JSON.parse(json);return true;}
function present(){
  if(guide)return !guide.isDestroyed();
  if(!configuration||closed||creating)return false;
  creating=true;
  locatorModule.createNativeSystemSettingsLocator({loadObjcModule:async()=>objc}).then(locate=>
    ui.createNativeAccessibilitySetupWindow({appPath:'/Applications/ChatGPT.app',copy:configuration.copy,
      layoutDirection:configuration.layoutDirection,loadObjcModule:async()=>objc,locateSettings:locate,
      prepareSettings:()=>locate.prepareHandoff?.(),
      activate:()=>$.NSApplication.sharedApplication.activateIgnoringOtherApps(true),
      canPresent:()=>!closed,
      onHandoff:options=>ui.runNativePermissionHandoff({...options,onError:error=>emit('error',String(error))}),
      onBack:options=>ui.runNativePermissionHandoff({...options,onError:error=>emit('error',String(error))})})
  ).then(value=>{
    creating=false;if(closed){value?.close();return;}if(!value)throw Error('Permission window unavailable');
    guide=value;
    guide.choice.then(choice=>emit(choice==='repair'?'allow':'later'));
    guide.onRetry(()=>emit('retry'));
    guide.onClose(()=>{if(!closed)emit('close');});
  }).catch(error=>{creating=false;emit('error',String(error));});
  return Boolean(guide);
}
function isready(){return Boolean(guide&&!guide.isDestroyed()&&!closed);}
function setstate(json){const value=JSON.parse(json);if(!guide||closed)throw Error('Permission guide is not ready');guide.setState(value.state);return true;}
function drainEvents(){return JSON.stringify(events.splice(0));}
function close(){if(closed)return true;closed=true;guide?.close();for(const id of [...timers.keys()])clearTimeout(id);events.length=0;return true;}
