#!/usr/bin/env swift
//
// dmg-background.swift — draws the Android18 DMG installer background.
//
// Usage: dmg-background.swift <out-1x.png> <out-2x.png> [version]
//
// build-dmg.sh combines both PNGs into a Retina TIFF (`tiffutil
// -cathidpicheck`); a checked-in .DS_Store template pre-arranges the
// 660x400 Finder window over it. The design is deliberately
// *position-independent*: nothing here points at
// where Finder places the icons (on some macOS builds those positions do
// not stick, and painted arrows then disagree with the live layout), so
// the instruction pill at the bottom carries the drag direction instead.
//
// Colors mirror the app palette (crates/app/src/theme.rs):
// slate-50 #F8FAFC · slate-100 #F1F5F9 · slate-200 #E2E8F0 ·
// slate-400 #94A3B8 · slate-500 #64748B.

import CoreGraphics
import CoreText
import Foundation
import ImageIO
import UniformTypeIdentifiers

let W: CGFloat = 660
let H: CGFloat = 400

func fail(_ message: String) -> Never {
    FileHandle.standardError.write((message + "\n").data(using: .utf8)!)
    exit(1)
}

func rgb(_ hex: UInt32, _ alpha: CGFloat = 1) -> CGColor {
    CGColor(srgbRed: CGFloat((hex >> 16) & 0xFF) / 255,
            green: CGFloat((hex >> 8) & 0xFF) / 255,
            blue: CGFloat(hex & 0xFF) / 255,
            alpha: alpha)
}

let slate50 = rgb(0xF8FAFC)
let slate100 = rgb(0xF1F5F9)
let slate200 = rgb(0xE2E8F0)
let slate400 = rgb(0x94A3B8)
let slate500 = rgb(0x64748B)
let slate600 = rgb(0x475569)

/// CG's origin is bottom-left; everything here is authored from the top.
func pt(_ x: CGFloat, _ yTop: CGFloat) -> CGPoint { CGPoint(x: x, y: H - yTop) }

func topRect(_ x: CGFloat, _ yTop: CGFloat, _ w: CGFloat, _ h: CGFloat) -> CGRect {
    CGRect(x: x, y: H - yTop - h, width: w, height: h)
}

enum Anchor { case left, center, right }

func drawText(_ ctx: CGContext, _ text: String, _ size: CGFloat, _ tint: CGColor,
              _ x: CGFloat, _ yTop: CGFloat, _ anchor: Anchor, bold: Bool = false) {
    var font = CTFontCreateUIFontForLanguage(.system, size, "en" as CFString)
        ?? CTFontCreateWithName("HelveticaNeue" as CFString, size, nil)
    if bold,
       let boldFont = CTFontCreateCopyWithSymbolicTraits(font, 0, nil, .traitBold, []) {
        font = boldFont
    }
    let attrs = [kCTFontAttributeName: font,
                 kCTForegroundColorAttributeName: tint] as CFDictionary
    guard let attr = CFAttributedStringCreate(nil, text as CFString, attrs) else {
        fail("dmg-background: could not typeset \"\(text)\"")
    }
    let line = CTLineCreateWithAttributedString(attr)
    let width = CGFloat(CTLineGetTypographicBounds(line, nil, nil, nil))
    let penX: CGFloat
    switch anchor {
    case .left: penX = x
    case .center: penX = x - width / 2
    case .right: penX = x - width
    }
    ctx.saveGState()
    ctx.translateBy(x: penX, y: H - yTop)
    ctx.scaleBy(x: 1, y: -1)
    CTLineDraw(line, ctx)
    ctx.restoreGState()
}

func render(_ scale: Int, version: String) -> CGImage {
    guard let space = CGColorSpace(name: CGColorSpace.sRGB),
        let ctx = CGContext(data: nil, width: Int(W) * scale, height: Int(H) * scale,
                            bitsPerComponent: 8, bytesPerRow: 0, space: space,
                            bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue) else {
        fail("dmg-background: could not create \(scale)x context")
    }
    ctx.scaleBy(x: CGFloat(scale), y: CGFloat(scale))

    // Slate-50 → slate-100 vertical wash.
    guard let gradient = CGGradient(colorsSpace: space, colors: [slate50, slate100] as CFArray,
                                    locations: [0, 1]) else {
        fail("dmg-background: could not create gradient")
    }
    ctx.drawLinearGradient(gradient, start: pt(0, 0), end: pt(0, H), options: [])

    // Brand band: wordmark left, version right (baselines aligned).
    drawText(ctx, "Android18", 20, slate600, 28, 40, .left, bold: true)
    if !version.isEmpty {
        drawText(ctx, "Android18 \(version)", 11, slate400, W - 28, 40, .right)
    }

    // Self-contained instruction pill. It replaces the old painted arrow:
    // an arrow between two painted drop zones only reads correctly when
    // Finder honors the icon positions, which not every macOS build does.
    // The pill names both endpoints and closes with a trailing arrow, so
    // the drag direction is right wherever the icons actually land.
    let pillW: CGFloat = 392
    let pillH: CGFloat = 44
    let pill = topRect((W - pillW) / 2, H - 64, pillW, pillH)
    ctx.setFillColor(rgb(0xFFFFFF, 0.85))
    ctx.setStrokeColor(slate200)
    ctx.setLineWidth(2)
    ctx.addPath(CGPath(roundedRect: pill, cornerWidth: pillH / 2, cornerHeight: pillH / 2,
                       transform: nil))
    ctx.drawPath(using: .fillStroke)
    drawText(ctx, "Drag Android18 to Applications  →", 13, slate500,
             W / 2, H - 64 + 27, .center)
    guard let image = ctx.makeImage() else {
        fail("dmg-background: could not snap \(scale)x image")
    }
    return image
}

func write(_ image: CGImage, _ path: String) {
    let url = URL(fileURLWithPath: path) as CFURL
    guard let dest = CGImageDestinationCreateWithURL(
        url, UTType.png.identifier as CFString, 1, nil) else {
        fail("dmg-background: could not create \(path)")
    }
    CGImageDestinationAddImage(dest, image, nil)
    guard CGImageDestinationFinalize(dest) else {
        fail("dmg-background: could not write \(path)")
    }
}

let args = CommandLine.arguments
guard args.count == 3 || args.count == 4 else {
    fail("usage: dmg-background.swift <out-1x.png> <out-2x.png> [version]")
}
let version = args.count == 4 ? args[3] : ""
write(render(1, version: version), args[1])
write(render(2, version: version), args[2])
