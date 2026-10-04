# Photo clipping-group blending: bounded CPU correction

## Confirmed contract

Adobe documents that **Blend Clipped Layers As Group** applies the base layer's
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
an externally verified Photoshop oracle for every Fill/clip combination. Member
special Fill still uses the existing `blend_px_fill` path.

## Explicit compatibility boundary

This is **not complete Photoshop clipping-group parity**. The following retain
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
- The option-off opacity/Fill behavior. Adobe's text establishes retention of
  member modes, but does not settle the full off-state opacity/Fill matrix.

To settle these cases, obtain an actual Photoshop PSD and flattened reference
with a known color/blend space, fractional base alpha, translucent backdrop,
base/member mode and Fill/Opacity controls, plus separate toggles for interior
styles and masks. Style fixtures must distinguish below/outside effects, interior
effects, clip member ordering, and original shape from effect-expanded alpha.
Group fixtures must include a child blend or adjustment that reads the external
backdrop. Do not label CPU/GPU self-agreement as independent Photoshop evidence.

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

The compute compositor and native viewport reject all visible enabled clipping stacks until
they implement the same stack envelope. Normal/100% opacity is not a safe exception:
fractional base alpha still must be applied only once. The native viewport records
a specific capability reason and uses its existing CPU fallback. This costs GPU
compositing acceleration for these scenes, rather than displaying different pixels
from CPU export. Ungrouped supported scenes retain GPU execution. Legacy internal
root-slot and pass-through shape tests remain to protect those fixes independently
of the new gate.

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
blends, Deep-knockout compatibility, GPU refusal, fallback presentation,
release/restoration, pixel edits, and reload. Runtime tests must be executed by the
integration build owner; formatting/source checks alone are not runtime proof.
