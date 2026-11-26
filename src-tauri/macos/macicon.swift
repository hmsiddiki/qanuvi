// Draws Qanuvi's Mac app icon: the logo on a dark rounded square on Apple's 1024 grid (824 px square, 100 px margin).
// Usage: macicon <logo.png> <out.png>
import AppKit

let args = CommandLine.arguments
guard args.count == 3, let logo = NSImage(contentsOfFile: args[1]) else { fatalError("usage: macicon logo.png out.png") }

let size = 1024
let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: size, pixelsHigh: size, bitsPerSample: 8,
                           samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB,
                           bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
NSGraphicsContext.current?.imageInterpolation = .high

let square = NSRect(x: 100, y: 100, width: 824, height: 824)
let radius: CGFloat = 185
let shape = NSBezierPath(roundedRect: square, xRadius: radius, yRadius: radius)

// Soft drop shadow like Apple's own icons.
NSGraphicsContext.saveGraphicsState()
let shadow = NSShadow()
shadow.shadowColor = NSColor.black.withAlphaComponent(0.35)
shadow.shadowBlurRadius = 18
shadow.shadowOffset = NSSize(width: 0, height: -8)
shadow.set()
NSColor(srgbRed: 0.08, green: 0.08, blue: 0.09, alpha: 1).setFill()
shape.fill()
NSGraphicsContext.restoreGraphicsState()

// Dark gradient (a little lighter at the top) inside the square.
let gradient = NSGradient(colors: [
    NSColor(srgbRed: 0.165, green: 0.157, blue: 0.200, alpha: 1),  // #2a2833, faint purple
    NSColor(srgbRed: 0.078, green: 0.078, blue: 0.086, alpha: 1),  // #141416, the window colour
])!
gradient.draw(in: shape, angle: -90)

// Thin light edge so the square reads on a dark Dock.
NSColor.white.withAlphaComponent(0.10).setStroke()
let edge = NSBezierPath(roundedRect: square.insetBy(dx: 1.5, dy: 1.5), xRadius: radius - 1.5, yRadius: radius - 1.5)
edge.lineWidth = 3
edge.stroke()

// The logo, centred, about three quarters of the square.
let side: CGFloat = 610
let logoRect = NSRect(x: (CGFloat(size) - side) / 2, y: (CGFloat(size) - side) / 2, width: side, height: side)
logo.draw(in: logoRect, from: .zero, operation: .sourceOver, fraction: 1)

NSGraphicsContext.restoreGraphicsState()
try! rep.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: args[2]))
print("wrote \(args[2])")
