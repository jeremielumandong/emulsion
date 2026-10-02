//! A focused OpenColorIO implementation in Rust: reads OCIO v1 and v2
//! configs, compiles colour space and display/view conversions into ops,
//! applies them exactly (for export) or through a baked 3D LUT (for the
//! canvas). No C++ library is involved.
//!
//! Supported transforms: ColorSpace, Look, Matrix, Exponent,
//! ExponentWithLinear, Log, LogAffine, LogCamera, CDL, Range, Group,
//! FixedFunction (ACES_Glow03/10, ACES_RedMod03/10, ACES_DarkToDim10,
//! ACES_GamutComp13), File (`.cube` 1D/3D/shaper+3D, `.spi1d`, `.spi3d`,
//! `.clf`/`.ctf` Matrix, LUT1D, LUT3D, Range, Log, Exponent, ASC_CDL) and
//! the Builtin styles in [`BUILTIN_STYLES`] (ACES AP0/AP1 conversions,
//! ACEScct/ACEScc/ACEScg, the ACES 1.0 SDR output transforms, the common
//! display encodings). Anything else (Grading*, ExposureContrast,
//! DisplayView, Allocation, ACES 2.0 and HDR output transforms, camera log
//! builtins, inverse 3D LUTs) compiles to an [`Error::Unsupported`] naming
//! it, never to silently wrong colour.

mod aces;
mod bake;
mod clf;
mod config;
mod lut;
mod ops;
mod processor;
mod transform;

#[cfg(test)]
mod tests;

pub use aces::{
    AP0, AP1, BUILTIN_STYLES, P3_D65, Primaries, REC709, REC2020, conversion, rgb_to_xyz,
    to_xyz_d65,
};
pub use bake::{BakedLut, Shaper};
pub use config::{
    ColorSpace, Config, Display, Look, Reference, USE_DISPLAY_NAME, View, ViewTransform,
};
pub use lut::{Lut1d, Lut3d};
pub use ops::Processor;
pub use transform::{Direction, Interpolation, LogParams, NegativeStyle, Transform};

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("OpenColorIO config: {0}")]
    Config(String),
    #[error("Unsupported in OpenColorIO here: {0}")]
    Unsupported(String),
    #[error("LUT: {0}")]
    Lut(String),
    #[error("{0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Emulsion's built-in config, used when no other is chosen: ACES2065-1 as
/// the scene reference, ACEScg, ACEScct, linear Rec.709 and sRGB texture,
/// with sRGB and Rec.1886/Rec.709 displays showing an ACES 1.0 SDR video
/// view and an un-tone-mapped view. Every transform is a builtin or an
/// analytic curve, so it needs no LUT files.
pub const BUILTIN_CONFIG: &str = r#"ocio_profile_version: 2
name: emulsion-builtin
description: Emulsion's built-in ACES config (no LUT files).
roles:
  aces_interchange: ACES2065-1
  cie_xyz_d65_interchange: CIE-XYZ-D65
  scene_linear: ACEScg
  rendering: ACEScg
  compositing_linear: ACEScg
  compositing_log: ACEScct
  color_timing: ACEScct
  color_picking: sRGB - Texture
  texture_paint: sRGB - Texture
  matte_paint: sRGB - Texture
  data: Raw
  default: sRGB - Texture
shared_views:
  - !<View> {name: Un-tone-mapped, view_transform: Un-tone-mapped, display_colorspace: <USE_DISPLAY_NAME>}
displays:
  sRGB - Display:
    - !<View> {name: ACES 1.0 - SDR Video, view_transform: ACES 1.0 - SDR Video, display_colorspace: <USE_DISPLAY_NAME>}
    - !<Views> [Un-tone-mapped]
    - !<View> {name: Raw, colorspace: Raw}
  Rec.1886 Rec.709 - Display:
    - !<View> {name: ACES 1.0 - SDR Video, view_transform: ACES 1.0 - SDR Video, display_colorspace: <USE_DISPLAY_NAME>}
    - !<Views> [Un-tone-mapped]
    - !<View> {name: Raw, colorspace: Raw}
default_view_transform: Un-tone-mapped
view_transforms:
  - !<ViewTransform>
    name: ACES 1.0 - SDR Video
    description: The ACES 1.0 reference rendering and SDR video output.
    from_scene_reference: !<BuiltinTransform> {style: ACES-OUTPUT - ACES2065-1_to_CIE-XYZ-D65 - SDR-VIDEO_1.0}
  - !<ViewTransform>
    name: Un-tone-mapped
    description: Colorimetric, no tone mapping.
    from_scene_reference: !<BuiltinTransform> {style: UTILITY - ACES-AP0_to_CIE-XYZ-D65_BFD}
looks:
  - !<Look>
    name: ACES 1.3 Reference Gamut Compression
    process_space: ACES2065-1
    transform: !<BuiltinTransform> {style: ACES-LMT - ACES 1.3 Reference Gamut Compression}
display_colorspaces:
  - !<ColorSpace>
    name: CIE-XYZ-D65
    aliases: [cie_xyz_d65]
    family: ""
    encoding: display-linear
    description: The display reference, CIE XYZ with a D65 white.
  - !<ColorSpace>
    name: sRGB - Display
    aliases: [srgb_display]
    family: Display
    encoding: sdr-video
    description: sRGB monitor (piecewise sRGB curve).
    from_display_reference: !<BuiltinTransform> {style: DISPLAY - CIE-XYZ-D65_to_sRGB}
  - !<ColorSpace>
    name: Rec.1886 Rec.709 - Display
    aliases: [rec1886_rec709_display]
    family: Display
    encoding: sdr-video
    description: Rec.709 video monitor (gamma 2.4).
    from_display_reference: !<BuiltinTransform> {style: DISPLAY - CIE-XYZ-D65_to_REC.1886-REC.709}
colorspaces:
  - !<ColorSpace>
    name: ACES2065-1
    aliases: [aces2065_1, ACES - ACES2065-1, lin_ap0]
    family: ACES
    encoding: scene-linear
    description: The scene reference, AP0 primaries.
  - !<ColorSpace>
    name: ACEScg
    aliases: [ACES - ACEScg, lin_ap1]
    family: ACES
    encoding: scene-linear
    description: Linear AP1, the usual rendering space.
    to_scene_reference: !<BuiltinTransform> {style: ACEScg_to_ACES2065-1}
  - !<ColorSpace>
    name: ACEScct
    aliases: [ACES - ACEScct, acescct_ap1]
    family: ACES
    encoding: log
    description: Log AP1 with a linear toe, for grading.
    to_scene_reference: !<BuiltinTransform> {style: ACEScct_to_ACES2065-1}
  - !<ColorSpace>
    name: Linear Rec.709 (sRGB)
    aliases: [lin_rec709_srgb, lin_srgb, Utility - Linear - sRGB]
    family: Utility
    encoding: scene-linear
    description: Linear light with Rec.709/sRGB primaries.
    to_scene_reference: !<GroupTransform>
      children:
        - !<BuiltinTransform> {style: UTILITY - ACES-AP1_to_LINEAR-REC709_BFD, direction: inverse}
        - !<BuiltinTransform> {style: ACEScg_to_ACES2065-1}
  - !<ColorSpace>
    name: sRGB - Texture
    aliases: [srgb_tx, Utility - sRGB - Texture, srgb_texture]
    family: Utility
    encoding: sdr-video
    description: sRGB-encoded Rec.709, as most painted and photographic images are.
    to_scene_reference: !<GroupTransform>
      children:
        - !<ExponentWithLinearTransform> {gamma: 2.4, offset: 0.055}
        - !<BuiltinTransform> {style: UTILITY - ACES-AP1_to_LINEAR-REC709_BFD, direction: inverse}
        - !<BuiltinTransform> {style: ACEScg_to_ACES2065-1}
  - !<ColorSpace>
    name: Raw
    aliases: [raw, Utility - Raw]
    family: Utility
    encoding: data
    isdata: true
    description: Data that is never colour managed.
"#;
