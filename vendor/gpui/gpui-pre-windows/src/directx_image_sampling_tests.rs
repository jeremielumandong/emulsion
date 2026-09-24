// Modified by Emulsion: native image-atlas sampling regression coverage.
use super::*;
use core::prelude::v1::test;
use windows::Win32::Foundation::HMODULE;

// Use the shipping vertex/pixel shaders and sampler on WARP, avoiding any
// display or adapter requirement. Every texel outside the tile is a sentinel.
#[test]
fn enlarged_image_tiles_do_not_sample_atlas_neighbors() -> Result<()> {
    let mut device = None;
    let mut context = None;
    unsafe {
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_WARP,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
    }
    let device = device.context("WARP device")?;
    let context = context.context("WARP context")?;
    let globals = DirectXGlobalElements::new(&device)?;
    update_buffer(
        &context,
        globals.global_params_buffer.as_ref().unwrap(),
        &[GlobalParams {
            viewport_size: [16.0, 16.0],
            ..Default::default()
        }],
    )?;
    update_batch_start(&context, globals.batch_params_buffer.as_ref().unwrap(), 0)?;
    unsafe {
        let constants = [
            globals.global_params_buffer.clone(),
            globals.batch_params_buffer.clone(),
        ];
        context.VSSetConstantBuffers(0, Some(&constants));
        context.PSSetConstantBuffers(0, Some(&constants));
        context.RSSetViewports(Some(&[D3D11_VIEWPORT {
            Width: 16.0,
            Height: 16.0,
            MaxDepth: 1.0,
            ..Default::default()
        }]));
        let mut rasterizer = None;
        device.CreateRasterizerState(
            &D3D11_RASTERIZER_DESC {
                FillMode: D3D11_FILL_SOLID,
                CullMode: D3D11_CULL_NONE,
                DepthClipEnable: true.into(),
                ..Default::default()
            },
            Some(&mut rasterizer),
        )?;
        context.RSSetState(rasterizer.as_ref());
    }
    let mut blend = None;
    let mut blend_desc = D3D11_BLEND_DESC::default();
    blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
        SrcBlend: D3D11_BLEND_ONE,
        DestBlend: D3D11_BLEND_ZERO,
        BlendOp: D3D11_BLEND_OP_ADD,
        SrcBlendAlpha: D3D11_BLEND_ONE,
        DestBlendAlpha: D3D11_BLEND_ZERO,
        BlendOpAlpha: D3D11_BLEND_OP_ADD,
        RenderTargetWriteMask: D3D11_COLOR_WRITE_ENABLE_ALL.0 as u8,
        ..Default::default()
    };
    unsafe {
        device.CreateBlendState(&blend_desc, Some(&mut blend))?;
    }
    let mut pipeline = PipelineState::<PolychromeSprite>::new(
        &device,
        "image sampling test",
        ShaderModule::PolychromeSprite,
        1,
        blend.unwrap(),
    )?;

    let red = [1.0, 0.0, 0.0, 1.0];
    let green = [0.0, 1.0, 0.0, 1.0];
    let blue = [0.0, 0.0, 1.0, 1.0];
    let white = [1.0, 1.0, 1.0, 1.0];
    let translucent = [0.2, 0.4, 0.6, 0.25];
    let mut texels = [[1.0_f32, 0.0, 1.0, 0.0]; 16];
    texels[5] = red;
    texels[6] = green;
    texels[9] = blue;
    texels[10] = white;
    texels[15] = translucent;
    let atlas = image_texture(
        &device,
        4,
        D3D11_BIND_SHADER_RESOURCE.0 as u32,
        Some(&texels),
    )?;
    let mut atlas_view = None;
    unsafe {
        device.CreateShaderResourceView(&atlas, None, Some(&mut atlas_view))?;
    }
    let target = image_texture(&device, 16, D3D11_BIND_RENDER_TARGET.0 as u32, None)?;
    let mut target_view = None;
    unsafe {
        device.CreateRenderTargetView(&target, None, Some(&mut target_view))?;
        context.OMSetRenderTargets(Some(&[target_view.clone()]), None);
    }

    let mut render =
        |origin: i32, extent: i32, fragment: Option<ID3D11PixelShader>| -> Result<Vec<[f32; 4]>> {
            if let Some(fragment) = fragment {
                pipeline.fragment = fragment;
            }
            let bounds = Bounds::new(
                point(ScaledPixels(0.0), ScaledPixels(0.0)),
                size(ScaledPixels(16.0), ScaledPixels(16.0)),
            );
            let sprite = PolychromeSprite {
                order: 0,
                pad: 0,
                grayscale: false.into(),
                opacity: 1.0,
                bounds,
                content_mask: ContentMask { bounds },
                corner_radii: Corners::default(),
                tile: AtlasTile {
                    texture_id: AtlasTextureId {
                        index: 0,
                        kind: AtlasTextureKind::Polychrome,
                    },
                    tile_id: TileId(0),
                    padding: 0,
                    bounds: Bounds::new(
                        point(DevicePixels(origin), DevicePixels(origin)),
                        size(DevicePixels(extent), DevicePixels(extent)),
                    ),
                },
            };
            pipeline.update_buffer(&device, &context, &[sprite])?;
            unsafe {
                context.ClearRenderTargetView(target_view.as_ref().unwrap(), &[0.0; 4]);
            }
            pipeline.draw_with_texture(
                &context,
                std::slice::from_ref(&atlas_view),
                std::slice::from_ref(&globals.sampler),
                1,
            )?;
            read_pixels(&device, &context, &target)
        };
    let pixels = render(1, 2, None)?;
    for (x, y, expected) in [(0, 0, red), (15, 0, green), (0, 15, blue), (15, 15, white)] {
        assert_pixel(pixels[y * 16 + x], expected, "magnified corner");
    }
    // Each source texel covers eight destination pixels. At (5.5, 3.5),
    // bilinear filtering is 3/16 of the way from red to green. This rejects
    // an inset-vertex-UV fix that removes seams by shrinking the whole image.
    assert_pixel(
        pixels[3 * 16 + 5],
        [0.8125, 0.1875, 0.0, 1.0],
        "interior interpolation",
    );
    for pixel in render(1, 1, None)? {
        assert_pixel(pixel, red, "single-texel crop");
    }
    // An atlas-edge crop must also preserve straight alpha without wrapping
    // to texels at the opposite edge of the texture.
    for pixel in render(3, 1, None)? {
        assert_pixel(pixel, translucent, "translucent atlas-edge crop");
    }
    // Negative control compiles the previous sampling behavior in memory.
    // It proves the fixture catches the bug without editing production files.
    let old_pixels = render(1, 2, Some(unclamped_fragment(&device)?))?;
    assert_pixel(
        old_pixels[0],
        [1.0, 0.0, 0.68359375, 0.31640625],
        "unclamped negative control",
    );
    eprintln!(
        "Native atlas corner RGBA: unclamped {:?}; fixed {:?}",
        old_pixels[0], pixels[0]
    );
    Ok(())
}

fn unclamped_fragment(device: &ID3D11Device) -> Result<ID3D11PixelShader> {
    use windows::{Win32::Graphics::Direct3D::Fxc::D3DCompile, core::s};
    let source = include_str!("shaders.hlsl").replace(
        "#include \"alpha_correction.hlsl\"",
        include_str!("alpha_correction.hlsl"),
    );
    let sampling = "t_sprite.Sample(s_sprite, tile_position)";
    assert!(
        source.contains(sampling),
        "negative control must bypass the image clamp"
    );
    let source = source.replace(sampling, "t_sprite.Sample(s_sprite, input.tile_position)");
    let mut code = None;
    let mut errors = None;
    unsafe {
        let result = D3DCompile(
            source.as_ptr().cast(),
            source.len(),
            s!("unclamped-atlas-test"),
            None,
            None,
            s!("polychrome_sprite_fragment"),
            s!("ps_4_1"),
            0,
            0,
            &mut code,
            Some(&mut errors),
        );
        if let Err(error) = result {
            let details = errors
                .as_ref()
                .map(|blob| {
                    String::from_utf8_lossy(std::slice::from_raw_parts(
                        blob.GetBufferPointer().cast::<u8>(),
                        blob.GetBufferSize(),
                    ))
                    .into_owned()
                })
                .unwrap_or_default();
            anyhow::bail!("negative-control shader: {error}: {details}");
        }
        let code = code.context("negative-control bytecode")?;
        create_fragment_shader(
            device,
            std::slice::from_raw_parts(code.GetBufferPointer().cast::<u8>(), code.GetBufferSize()),
        )
    }
}

fn image_texture(
    device: &ID3D11Device,
    extent: u32,
    bind: u32,
    texels: Option<&[[f32; 4]]>,
) -> Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: extent,
        Height: extent,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_R32G32B32A32_FLOAT,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_DEFAULT,
        BindFlags: bind,
        ..Default::default()
    };
    let data = texels.map(|pixels| D3D11_SUBRESOURCE_DATA {
        pSysMem: pixels.as_ptr().cast(),
        SysMemPitch: extent * 16,
        SysMemSlicePitch: 0,
    });
    let mut texture = None;
    unsafe {
        device.CreateTexture2D(
            &desc,
            data.as_ref().map(|data| data as *const _),
            Some(&mut texture),
        )?;
    }
    texture.context("test texture")
}

fn read_pixels(
    device: &ID3D11Device,
    context: &ID3D11DeviceContext,
    target: &ID3D11Texture2D,
) -> Result<Vec<[f32; 4]>> {
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    unsafe {
        target.GetDesc(&mut desc);
    }
    desc.Usage = D3D11_USAGE_STAGING;
    desc.BindFlags = 0;
    desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    let mut staging = None;
    unsafe {
        device.CreateTexture2D(&desc, None, Some(&mut staging))?;
    }
    let staging = staging.context("readback texture")?;
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        context.CopyResource(&staging, target);
        context.Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))?;
    }
    let mut pixels = Vec::with_capacity((desc.Width * desc.Height) as usize);
    // SAFETY: the mapped staging allocation contains Height rows, each with
    // RowPitch bytes. A row holds Width RGBA float pixels and remains mapped
    // until all rows have been copied below.
    unsafe {
        for row in 0..desc.Height as usize {
            let ptr = mapped
                .pData
                .cast::<u8>()
                .add(row * mapped.RowPitch as usize)
                .cast::<[f32; 4]>();
            pixels.extend_from_slice(std::slice::from_raw_parts(ptr, desc.Width as usize));
        }
        context.Unmap(&staging, 0);
    }
    Ok(pixels)
}

fn assert_pixel(actual: [f32; 4], expected: [f32; 4], case: &str) {
    for channel in 0..4 {
        assert!(
            (actual[channel] - expected[channel]).abs() < 0.002,
            "{case}: expected {expected:?}, got {actual:?}"
        );
    }
}
