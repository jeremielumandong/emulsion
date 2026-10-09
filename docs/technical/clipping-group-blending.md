# Photo clipping-group blending: bounded envelopes and GPU acceleration

## Confirmed contract

The format vendor's documentation states that **Blend Clipped Layers As Group** applies the base layer's
blend mode to the clipping stack, and that clipped layers inherit base opacity:

- [Layer opacity and blending, Group blend effects](https://helpx.adobe.com/photoshop/using/layer-opacity-blending.html#group_blend_effects)
- [Create and manage clipping masks](https://helpx.adobe.com/photoshop/desktop/create-masks/layer-masks/create-and-manage-clipping-masks.html)

For a contiguous style-free stack rooted in a pixel, fill, or ordinary isolated
group layer, the CPU compositor now
separates the unfilled base shape from the internal color stack. It normalizes the
base color, composites members with their own authored modes/Opacity/Fill, applies
the base shape once, then applies the base mode and Opacity once to the complete
result. This avoids both per-member mode replacement and repeated fractional-edge
coverage. Placement, mip sampling, combined raster/vector masks, rectangle clips,
and the effective Smart Filter pixels come from the existing source-sampling path.
For an isolated-group root, children are evaluated against transparency with
their own blend modes and masks before capturing the group shape; the root mask
is included once. Stacks containing any root/member/descendant knockout remain
compatibility-gated. These stacks also work recursively inside ordinary and
pass-through parent groups.
Hidden intermediate clipped layers do not break root links; a hidden root hides
all members. No authored document state or native-file schema changes.

Base Fill remains the base-only interior contribution; zero Fill does not erase
its unfilled clipping shape. This is an application compatibility invariant, not
an externally verified reference-application oracle for every Fill/clip combination. Member
special Fill still uses the existing `blend_px_fill` path.

## Explicit compatibility boundary

This is **not complete clipping-group parity with other editors**. The following retain
existing CPU rendering, and the viewport capability diagnostic states why:

- Any knockout on the root, member, or recursively nested descendant.
  Normalizing Fill before knockout coverage would change a transparent-member
  no-op; preserve the established punch propagation until separately implemented.
- Any styled appearance on a root, clipped member, or recursively nested group
  descendant, including hidden StyledGroup nodes present in the render tree.
  Invisible authored layers may not expand into StyledGroup render nodes.
  Interior Effects As Group off is also unresolved. A root wrapper can have Normal mode and Fill 1 even when the
  authored base does not; a member's outer/backdrop-dependent effects must not
  silently receive a new normalized backdrop without verified effect ordering.
- Pass-through-group, derived clipped-group, and adjustment bases.
  Pass-through/adjustment appearance can depend on the real backdrop outside the
  alpha obtained by rendering the base in isolation.
- The eight special nonlinear Fill modes when base Fill is below 100%. Replacing
  their Fill operation with ordinary alpha attenuation changes even an otherwise
  transparent-member no-op.
- Noncontiguous links to an earlier sibling. Such native links are legal; grouping
  across an unrelated sibling must not silently change its position in the stack.
- The option-off opacity/Fill behavior. The vendor's text establishes retention of
  member modes, but does not settle the full off-state opacity/Fill matrix.

To settle these cases, obtain an application-authored PSD and flattened reference
with a known color/blend space, fractional base alpha, translucent backdrop,
base/member mode and Fill/Opacity controls, plus separate toggles for interior
styles and masks. Style fixtures must distinguish below/outside effects, interior
effects, clip member ordering, and original shape from effect-expanded alpha.
Group fixtures must include a child blend or adjustment that reads the external
backdrop. Do not label CPU/GPU self-agreement as independent reference-application evidence.

## Design page-background compatibility

Existing native Design page backgrounds deliberately encode their invisible
boundary as a default-blending path at Opacity 0. That is a shape-only Design
role, not a Photo clipping-stack opacity control. The document compiler lowers
only that metadata-identified boundary into an empty appearance with its original
path and masks retained as the independent clipping source. The derived node uses
a unit envelope and separate-member blending so Normal, Multiply and other image
modes continue to see the real page backdrop. Native source fields, role metadata,
visibility, saved files and export/bleed boundary recognition remain unchanged.
An untagged Photo layer or an explicitly non-default boundary does not receive
this compatibility lowering. The existing advanced/independent-appearance GPU
fallback applies to the derived node; no accelerated parity is claimed.

Shape-only background frames are not portable through object paste because all
object-paste destinations intentionally drop the page role. Capture rejects a
selection containing the role-identified boundary and image together, including
partial frame selections that would otherwise lose metadata. Paste also rejects
previously captured complete-role fragments before starting a transaction. Native
Copy preflights capture before writing the clipboard; Cut already preflights it
before deleting sources. Image-only copying keeps its established unclipped-image
behavior. Ordinary Photo stacks and already role-stripped legacy documents are
not inferred or migrated. A rectangular layout-frame substitution was rejected:
it changes media-frame crop controls, rotation/reshape rules and visibility
semantics. No schema or lossy conversion is added by this compatibility guard.

## Acceleration

The compute compositor and native viewport accelerate contiguous clipping stacks
whose root, members, and descendants are style-free pixel/fill/isolated-group
nodes with default `BlendingOptions`. Each compiler pushes an isolated stack,
renders the root at unit Opacity with Normal blending through its existing source
and mask paths, saves the resulting alpha, and normalizes its interior. Members
then blend without multiplying that root alpha again. The final pop applies the
saved shape, root Opacity, and authored root blend once. Masked group roots and
members, fractional antialiasing, and nested eligible envelopes use the same
contract as the CPU renderer. Native vectors remain device-resolution, but each
authored layer inside an envelope (including nested group descendants) has its
own Vello run. This avoids merging fractional coverage in Vello's sRGB color
space before the compositor performs linear source-over. Existing vector target
limits (128 MiB and hardware array-layer limits) still apply. The composite cache
never splits an envelope around a vector member or a saved-alpha dependency.

This is a bounded subset, not all-group or layer-effects acceleration. Advanced
Fill/options, knockout, styles, derived groups, adjustment bases or members,
pass-through bases or members, and noncontiguous lower-sibling links continue to
use CPU fallback. Hidden unrelated siblings and hidden later links do not make a
noncontiguous stack eligible. These restrictions also apply to descendants of a
stack root/member. An ordinary or pass-through parent outside the clipping stack
can still contain an eligible stack. Native Dissolve remains CPU-only; the tile
compute path retains its existing Dissolve implementation. Rectangle-clipped tile
compute scenes retain their existing fallback.

Each eligible stack is limited to 64 render nodes with bounded descendant depth.
The compilers additionally enforce their shader stack and resource budgets,
including envelope nesting: the tile path has 64 alpha slots, 512 commands, and
64 MiB of sampled source/lookup data; the native canvas has 16 clipping-root alpha
slots and its existing atlas budget. Both have a 16-entry shader group stack.
Oversized or otherwise unsupported scenes fall back as a whole. Automatic tile
routing still uses its existing transfer-cost heuristic: eligible Normal-only or
linear-only stacks can remain on CPU unless force/software mode requests GPU.
Direct `render_tile_gpu` numerical tests prove the eligible kernel, not default
dispatch or a hardware speedup. The routing gate is unchanged.

GPU tests require actual execution for eligible stacks; fallback-only fixtures
remain separate. Hosted viewport coverage checks initial load, eligible clipping
creation/opacity edits/release/restoration on GPU, then a real advanced-member-Fill
GPU-to-CPU handoff. Fallback retries retain the last successful GPU frame only
until current-revision CPU coverage arrives. Legacy root-slot and pass-through
shape tests remain to protect their fallback behavior independently.

## Independent numerical acceptance

Linear-space, opaque backdrop `D=(.5,.25,.75)`, base `B=(.2,.4,.6)`, clipped
`C=(.8,.2,.4)`, base Multiply:

- Opaque Normal member: ON `D*C=(.4,.05,.3)`; OFF `C=(.8,.2,.4)`.
- Base Opacity 50%: ON `D*.5+(D*C)*.5=(.45,.15,.525)`; Opacity 0 gives `D`.
- Opaque Screen member: internal `1-(1-B)*(1-C)=(.84,.52,.76)`, then base Multiply
  gives `(.42,.13,.57)`. This rejects mode replacement and double nonlinear blends.
- Intrinsic base alpha .5 and member alpha .5: internal color is
  `.5*B+.5*C=(.5,.3,.5)`. On `D`, output is `(.375,.1625,.5625,1)`; on transparent
  backdrop it is `(.25,.15,.25,.5)`. The old per-member compositor inflated alpha
  to .625 on the latter fixture.

Tests additionally cover masks, rectangle clips, placement, member opacity/Fill,
zero base Fill, hidden intermediate/root nodes, adjustment members, nested stacks,
transparent-member special-Fill compatibility, isolated-group root masks/child
blends, Deep-knockout compatibility, bounded GPU execution and unsupported-scene fallback,
nested envelope depth/slot limits, cache boundaries, fallback presentation,
release/restoration, pixel edits, and reload. Runtime tests must be executed by the
integration build owner; formatting/source checks alone are not runtime proof.
