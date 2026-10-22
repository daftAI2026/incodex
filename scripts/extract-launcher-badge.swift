/**
 * [INPUT]: 依赖Apple黑白matte导出与单一胶囊布局。
 * [OUTPUT]: 提取透明铭牌PNG，不包含宿主图像。
 * [POS]: 开发资产构建的matte分离步骤，不参与产品启动。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
import Foundation
import CoreGraphics
import ImageIO
guard CommandLine.arguments.count == 5 else { fatalError("usage: extract-launcher-badge.swift black.png white.png layout.json output.png") }
struct Layout: Decodable { let canvas: Int; let x: Double; let y: Double; let width: Double; let height: Double; let radius: Double }
let layout = try JSONDecoder().decode(Layout.self, from: Data(contentsOf: URL(fileURLWithPath: CommandLine.arguments[3])))
precondition(layout.canvas == 1024)
func read(_ path:String)->[UInt8] {
 let image=CGImageSourceCreateImageAtIndex(CGImageSourceCreateWithURL(URL(fileURLWithPath:path) as CFURL,nil)!,0,nil)!
 var b=[UInt8](repeating:0,count:1024*1024*4)
 b.withUnsafeMutableBytes { m in
 let c=CGContext(data:m.baseAddress,width:1024,height:1024,bitsPerComponent:8,bytesPerRow:4096,space:CGColorSpace(name:CGColorSpace.sRGB)!,bitmapInfo:CGImageAlphaInfo.premultipliedLast.rawValue|CGBitmapInfo.byteOrder32Big.rawValue)!
 c.interpolationQuality = .high;c.draw(image,in:CGRect(x:0,y:0,width:1024,height:1024))
 };return b
}
let black=read(CommandLine.arguments[1]),white=read(CommandLine.arguments[2])
var mask=[UInt8](repeating:0,count:1024*1024)
mask.withUnsafeMutableBytes { m in
 let c=CGContext(data:m.baseAddress,width:1024,height:1024,bitsPerComponent:8,bytesPerRow:1024,space:CGColorSpaceCreateDeviceGray(),bitmapInfo:0)!
 c.translateBy(x:0,y:1024);c.scaleBy(x:1,y:-1);c.setFillColor(gray:1,alpha:1)
 c.addPath(CGPath(roundedRect:CGRect(x:layout.x,y:layout.y,width:layout.width,height:layout.height),cornerWidth:layout.radius,cornerHeight:layout.radius,transform:nil));c.fillPath()
}
var out=[UInt8](repeating:0,count:black.count)
for p in mask.indices where mask[p]>0 {
 let i=p*4
 let delta=(0..<3).map { Double(Int(white[i+$0])-Int(black[i+$0]))/255 }.reduce(0,+)/3
 let alpha=max(0,min(1,1-delta))
 out[i+3]=UInt8((alpha*255).rounded())
 for c in 0..<3 { out[i+c]=min(black[i+c],out[i+3]) }
}
let image=CGImage(width:1024,height:1024,bitsPerComponent:8,bitsPerPixel:32,bytesPerRow:4096,space:CGColorSpace(name:CGColorSpace.sRGB)!,bitmapInfo:CGBitmapInfo(rawValue:CGImageAlphaInfo.premultipliedLast.rawValue|CGBitmapInfo.byteOrder32Big.rawValue),provider:CGDataProvider(data:Data(out) as CFData)!,decode:nil,shouldInterpolate:true,intent:.defaultIntent)!
let path=CommandLine.arguments[4]
let d=CGImageDestinationCreateWithURL(URL(fileURLWithPath:path) as CFURL,"public.png" as CFString,1,nil)!
CGImageDestinationAddImage(d,image,nil);precondition(CGImageDestinationFinalize(d));print(path)
