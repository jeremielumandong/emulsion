#!/usr/bin/env swift
// Run from the repository root after a native build:
// swift scripts/test-metal-atlas.swift [path/to/generated/scene.h] [path/to/shaders.metal]
// Compiles the current Metal source, then reads back rendered atlas tile edges.
import Foundation
import Metal

func require(_ condition: Bool, _ message: String) {
    if !condition { fputs("FAIL: \(message)\n", stderr); exit(1) }
}
let root = URL(fileURLWithPath: FileManager.default.currentDirectoryPath)
let header: URL
if CommandLine.arguments.count > 1 {
    header = URL(fileURLWithPath: CommandLine.arguments[1])
} else {
    let build = root.appendingPathComponent("target/release/build")
    let candidates = try FileManager.default.contentsOfDirectory(at: build, includingPropertiesForKeys: nil)
        .filter { $0.lastPathComponent.hasPrefix("gpui-pre-apple-") }
        .map { $0.appendingPathComponent("out/scene.h") }
        .filter { FileManager.default.fileExists(atPath: $0.path) }
    require(!candidates.isEmpty, "Build the macOS app first, or pass a generated scene.h path")
    header = candidates[0]
}
guard let device = MTLCreateSystemDefaultDevice(), let queue = device.makeCommandQueue() else {
    fputs("FAIL: no Metal device available\n", stderr); exit(1)
}
// Have Metal initialize its own struct so this test does not duplicate Rust/C ABI layouts.
let fixture = """
kernel void make_test_sprite(device PolychromeSprite *sprites [[buffer(0)]],
                             constant float &width [[buffer(1)]]) {
    PolychromeSprite sprite = {};
    sprite.opacity = 1.0;
    sprite.bounds.size.width = width;
    sprite.bounds.size.height = width;
    sprite.content_mask.bounds = sprite.bounds;
    sprite.tile.bounds.origin.x = 2;
    sprite.tile.bounds.origin.y = 2;
    sprite.tile.bounds.size.width = 256;
    sprite.tile.bounds.size.height = 256;
    sprites[0] = sprite;
}
"""
let shaderURL = CommandLine.arguments.count > 2
    ? URL(fileURLWithPath: CommandLine.arguments[2])
    : root.appendingPathComponent("vendor/gpui/gpui-pre-apple/src/shaders.metal")
let source = try String(contentsOf: header, encoding: .utf8) + "\n" +
    String(contentsOf: shaderURL, encoding: .utf8) + "\n" + fixture
let library = try device.makeLibrary(source: source, options: nil)
let setupPipeline = try device.makeComputePipelineState(function: library.makeFunction(name: "make_test_sprite")!)
let descriptor = MTLRenderPipelineDescriptor()
descriptor.vertexFunction = library.makeFunction(name: "polychrome_sprite_vertex")
descriptor.fragmentFunction = library.makeFunction(name: "polychrome_sprite_fragment")
descriptor.colorAttachments[0].pixelFormat = .rgba8Unorm
let pipeline = try device.makeRenderPipelineState(descriptor: descriptor)
func texture(_ width: Int, _ usage: MTLTextureUsage) -> MTLTexture {
    let descriptor = MTLTextureDescriptor.texture2DDescriptor(pixelFormat: .rgba8Unorm, width: width, height: width, mipmapped: false)
    descriptor.usage = usage
    descriptor.storageMode = .shared
    return device.makeTexture(descriptor: descriptor)!
}
let atlasWidth = 260
var cases = 0
for value: UInt8 in [0, 255] {
    let atlas = texture(atlasWidth, .shaderRead)
    var pixels = [UInt8](repeating: 255, count: atlasWidth * atlasWidth * 4)
    for y in 0..<atlasWidth {
        for x in 0..<atlasWidth {
            let color = (2..<258).contains(x) && (2..<258).contains(y) ? value : 255 - value
            for channel in 0..<3 { pixels[(y * atlasWidth + x) * 4 + channel] = color }
        }
    }
    pixels.withUnsafeBytes {
        atlas.replace(region: MTLRegionMake2D(0, 0, atlasWidth, atlasWidth), mipmapLevel: 0, withBytes: $0.baseAddress!, bytesPerRow: atlasWidth * 4)
    }
    // Device-pixel-snapped 256px tiles: 77% zoom at 1x and Retina 2x,
    // plus the magnified tile fallback during viewport refresh.
    for width in [197, 394, 512] {
        let target = texture(width, .renderTarget)
        let sprites = device.makeBuffer(length: 256, options: .storageModeShared)!
        let command = queue.makeCommandBuffer()!
        let setup = command.makeComputeCommandEncoder()!
        setup.setComputePipelineState(setupPipeline)
        setup.setBuffer(sprites, offset: 0, index: 0)
        var size = Float(width)
        setup.setBytes(&size, length: 4, index: 1)
        setup.dispatchThreads(MTLSize(width: 1, height: 1, depth: 1), threadsPerThreadgroup: MTLSize(width: 1, height: 1, depth: 1))
        setup.endEncoding()
        let pass = MTLRenderPassDescriptor()
        pass.colorAttachments[0].texture = target
        pass.colorAttachments[0].loadAction = .clear
        pass.colorAttachments[0].storeAction = .store
        pass.colorAttachments[0].clearColor = MTLClearColor(red: 1, green: 0, blue: 1, alpha: 0)
        let render = command.makeRenderCommandEncoder(descriptor: pass)!
        render.setRenderPipelineState(pipeline)
        let vertices: [Float] = [0, 0, 1, 0, 0, 1, 1, 1]
        vertices.withUnsafeBytes { render.setVertexBytes($0.baseAddress!, length: $0.count, index: 0) }
        render.setVertexBuffer(sprites, offset: 0, index: 1)
        var viewport = SIMD2<UInt32>(repeating: UInt32(width))
        var atlasSize = SIMD2<UInt32>(repeating: UInt32(atlasWidth))
        render.setVertexBytes(&viewport, length: 8, index: 2)
        render.setVertexBytes(&atlasSize, length: 8, index: 3)
        render.setFragmentBuffer(sprites, offset: 0, index: 1)
        render.setFragmentTexture(atlas, index: 4)
        render.drawPrimitives(type: .triangleStrip, vertexStart: 0, vertexCount: 4)
        render.endEncoding()
        command.commit()
        command.waitUntilCompleted()
        require(command.status == .completed, "Metal execution failed: \(String(describing: command.error))")
        var output = [UInt8](repeating: 0, count: width * width * 4)
        output.withUnsafeMutableBytes {
            target.getBytes($0.baseAddress!, bytesPerRow: width * 4, from: MTLRegionMake2D(0, 0, width, width), mipmapLevel: 0)
        }
        for y in 0..<width {
            for x in 0..<width {
                let i = (y * width + x) * 4
                require(Array(output[i..<i+4]) == [value, value, value, 255],
                        "atlas bleed at (\(x), \(y)), width \(width), expected \(value), got \(Array(output[i..<i+4]))")
            }
        }
        cases += 1
    }
}
print("PASS: native Metal atlas sampling, \(cases) cases (77% at 1x/2x and magnified fallback; black/white tiles)")
