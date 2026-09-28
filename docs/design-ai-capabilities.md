# Design AI capabilities and availability

Design uses the same native image and assistant workflows as Photo mode. Generated
pixels are separate labeled layers; AI does not replace editable text, charts,
components or vectors with hidden screenshots.

| Workflow | Runtime requirement | Native behavior |
| --- | --- | --- |
| Select subject / remove background | Installed local matte model | Reports the missing model before starting; background removal preserves the original and produces a separate result. |
| Point/box selection | Installed supported segmentation model | Native selection with confidence; foreground/background points and box supported. |
| Remove a selected object | Local fill model, or the explicitly configured image provider | Requires a nonempty selection; preserves source and returns a new result layer. |
| Image generation / generative fill | Configured Local SD, OpenAI or Google image provider | Validates provider configuration before work. Fill sends selected image context to that provider. Cloud API usage is separate from the app. |
| Upscale, depth and face restoration | Corresponding installed local models | Missing models produce actionable setup messages; jobs support cancellation. |
| Design construction and editing | Configured assistant with native MCP tools | Authors real pages, text, shapes, charts, layouts, variables and components through ordinary native commands and history. |

`list_models` reports local model availability, download size and licensing;
`download_model` is an explicit setup action. Image provider keys remain in
Settings and are not included in templates, generated HTML, brand packs or native
Design exports. No provider subscription, model download or browser runtime is
implicitly bundled with a starter template.

Validation code lives in `editor/ai_tools.rs`, `editor/generate_ui.rs`,
`emulsion-ai` provider configuration, and the corresponding MCP model/image tools.
Native generation tests cover missing/empty selections, active-edit guards,
cancellation and rejection of stale results. Model quality and network-provider
availability require runtime acceptance with the actual configured provider; an
authoring control alone does not establish those capabilities.
