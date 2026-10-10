# Colour management

Emulsion manages colour in one of two ways, chosen under **Settings › Color
management**:

- **ICC** (the default). Pictures that carry an ICC profile (Display P3 phone
  photos, Adobe RGB camera JPEGs, ProPhoto TIFFs, CMYK files) are converted to
  sRGB as they are opened, so every document works in sRGB and exports are
  sRGB. Library has its own viewing and proofing profiles; see
  [Library develop](library-develop.md).
- **OpenColorIO**. The canvas, the storyboard Stage and the animatic player
  show pixels through an OpenColorIO (OCIO) display and view, such as the ACES
  1.0 SDR rendering, and image, movie and PDF exports are written in the
  display's colours or in a colour space you choose. ICC conversion of
  imported pictures still happens first.

Turning OpenColorIO off returns to ICC behaviour exactly: nothing is stored in
your documents except a storyboard's own working colour space, which is
ignored while OpenColorIO is off.

## How Emulsion reads pixels with OpenColorIO

Each pixel's stored value (0–1) is read as a colour in the **working colour
space**. With the built-in config the default is *sRGB - Texture*, which is
what painted and photographic pictures are, so the *Un-tone-mapped* view shows
them exactly as ICC does and the *ACES 1.0 - SDR Video* view shows them
through the ACES tone curve. Choosing *ACEScg* or *ACEScct* as the working
colour space treats the same values as linear or log ACES data.

The working colour space comes from, in order:

1. the storyboard's own (Settings › Color management › **this storyboard**, or
   the `set_ocio_config` tool), saved in the project;
2. the default in Settings (**working colour space**);
3. the config's `texture_paint`, `color_picking` or `default` role.

## Settings

| Setting | What it does |
| --- | --- |
| Colour management | **ICC** or **OpenColorIO**. |
| Config | **Built-in ACES**, **$OCIO** (the config the `OCIO` environment variable names) or **File…** (any `.ocio` config). A config file that changes on disk is read again. |
| Display, view | What the canvas and player show. A new display starts at its first view. |
| Look | **The view's own** looks, **None**, or one of the config's looks instead. |
| Working colour space | The default for documents that do not set one. |
| This storyboard | The open storyboard's working colour space (one Undo step), or **Use the default**. |
| Exports in | **As displayed** (the display and view, the default), or any colour space of the config. |

A name that is not in the config (after switching configs, say) is reported
under the section, and the canvas falls back to showing pixels unconverted
until it is fixed; exports refuse with the same message rather than writing
the wrong colours.

## The built-in config

The built-in config needs no files. It has:

- colour spaces **ACES2065-1** (the scene reference), **ACEScg**, **ACEScct**,
  **Linear Rec.709 (sRGB)**, **sRGB - Texture** and **Raw** (data, never
  converted);
- displays **sRGB - Display** and **Rec.1886 Rec.709 - Display**, each with the
  views **ACES 1.0 - SDR Video**, **Un-tone-mapped** and **Raw**;
- the look **ACES 1.3 Reference Gamut Compression**.

## Display and export

The canvas bakes the display transform into a 65³ 3D LUT (with a log shaper
when the working colour space is linear) applied to each tile with
tetrahedral interpolation, so the cost per pixel does not depend on the
transform. While OpenColorIO is on, the GPU canvas stands down for the tile
canvas, which applies the LUT. The player applies the same LUT to its
pictures before transitions and burn-in. Thumbnails (the board, the panel
strip and the light table) are not converted.

Exports run the exact transform, not the LUT:

- **Panel images** and **animatic movies and GIFs** are converted after the
  panel is flattened onto white. Burn-in text and the reference video are
  drawn after, unconverted. PNG and JPEG images converted this way carry no
  sRGB tag or ICC profile, since they are in the OCIO colour space.
- **Storyboard PDFs** place each converted panel as an image, so panels are
  not vector art while OpenColorIO is on.

## What the OpenColorIO support covers

Emulsion reads OCIO configs itself (no OpenColorIO library is involved):
profile versions 1 and 2, roles, colour spaces with aliases, families,
encodings and `isdata`, `displays`, `views`, `active_displays`,
`active_views`, `inactive_colorspaces`, `shared_views`, `view_transforms`,
`display_colorspaces` and `<USE_DISPLAY_NAME>`, looks (`+look`, `-look`,
several in a row), the `search_path` (relative to the config, list or
colon-separated) and environment variables in paths (`$VAR`, `${VAR}`, with
the config's `environment` defaults; `$OCIO_ACTIVE_DISPLAYS`,
`$OCIO_ACTIVE_VIEWS` and `$OCIO_INACTIVE_COLORSPACES` are honoured).

Transforms:

| Transform | Support |
| --- | --- |
| ColorSpace, Look, Group | Yes, in both directions. |
| Matrix, Exponent, ExponentWithLinear, Log, LogAffine, LogCamera, CDL, Range | Yes, with every negative-value style and both directions. |
| File | `.cube` (1D, 3D, or a 1D shaper and a 3D LUT; both common `.cube` keyword dialects), `.spi1d`, `.spi3d`, and `.clf`/`.ctf` with Matrix, LUT1D, LUT3D, Range, Log, Exponent and ASC_CDL nodes. Linear, tetrahedral and nearest interpolation. 1D LUTs invert; 3D LUTs do not. |
| FixedFunction | ACES_Glow03/10, ACES_RedMod03/10, ACES_DarkToDim10, ACES_GamutComp13. |
| Builtin | IDENTITY; the ACES AP0/AP1 utility matrices; ACEScct, ACEScc and ACEScg to ACES2065-1; the ACEScct curve; the Blue Light Artifact Fix and ACES 1.3 Gamut Compression LMTs; the ACES 1.0 SDR output transforms (SDR-CINEMA_1.0, SDR-VIDEO_1.0 and the 1.1 Rec.709- and P3-limited variants); display encodings to Rec.1886 (Rec.709, Rec.2020), gamma 2.2, sRGB, gamma 2.6 P3 (DCI, D65, D60), Display P3, ST 2084 (Rec.2100 PQ, P3-D65) and the PQ curves. |

Not supported, each refused with a message naming it: the Grading
transforms, ExposureContrast, DisplayView and Allocation transforms, the ACES
2.0 and HDR (1000–4000 nit and 108 nit cinema) output transforms, the D60 and
DCI simulation output transforms, camera log builtins (ARRI, Sony, RED,
Canon, Panasonic, Apple), ADX and ACESproxy, other FixedFunction styles,
`.cc`/`.ccc` CDL files and other LUT formats, half-domain and IndexMap CLF
LUTs, Cineon-style CTF Log parameters, inverse 3D LUTs, named transforms and
file and viewing rules (the working colour space is chosen in Settings
instead). A colour space that uses one of these fails on its own; the rest
of the config still works.

The ACES 1.0 output transforms evaluate the segmented-spline tone scales of
the ACES 1.0.3 CTL directly, where OpenColorIO fits them with B-splines;
results agree with OpenColorIO's reference values to within 2×10⁻⁴. The
inverse output transforms estimate the glow and red modifier weights from
their output, as OpenColorIO does, so a round trip is close but not exact
for saturated colours.

## For assistants

`describe_color_management` and `set_ocio_config` read and change these
settings, including the storyboard's working colour space; see
[MCP: storyboards](mcp/mcp-storyboard.md#colour-management).
