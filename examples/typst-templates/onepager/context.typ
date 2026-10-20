// Example branded one-pager — brand context (neutral sample branding).
//
// This is the per-brand half of the one-pager template: palette and the inputs
// read from `sys.inputs`. It is imported by `blocks.typ` (the shared block
// renderer, byte-identical across the sample and croit-branded template sets)
// and by `template.typ` (the render entrypoint). Replace the palette and logo
// below with your own, or point `[typst].templates_dir` at your own templates
// directory (see docs).
//
// A NON-letter branded document: title + optional subtitle + content, with a
// logo header and a generic page footer. Content arrives one of two ways, both
// separate from the template:
//   • `body`   — free-form native Typst markup (the original mechanism)
//   • `blocks` — a JSON array of typed content blocks (section, text, bullets,
//                stats, table, cards, image, quote, callout)
// The two compose: `body` renders first, then the blocks.

#let inputs = sys.inputs

// ---- Brand tokens (sample palette — swap for your own) ---------------------
#let c-primary = rgb("#2563EB")
#let c-accent = rgb("#0EA5E9")
#let ink = rgb("#0F172A")
#let muted = rgb("#64748B")
#let brand = gradient.linear(c-primary, c-accent)

// ---- Switches --------------------------------------------------------------
// Document language, for correct hyphenation/justification only ("en" default,
// "de" for a German one-pager).
#let lang = inputs.at("language", default: "en")

// ---- Content ---------------------------------------------------------------
#let title = inputs.at("title", default: "")
#let subtitle = inputs.at("subtitle", default: "")
#let body = inputs.at("body", default: "")
// The structured block system: a JSON array of {type, …} objects. Empty → the
// one-pager falls back to `body` alone.
#let blocks-json = inputs.at("blocks", default: "")
#let blocks = if blocks-json != "" { json(bytes(blocks-json)) } else { () }

// ---- Optional content image ------------------------------------------------
// The model may place ONE image in the one-pager (a photo, chart, diagram,
// screenshot, logo, …) at a chosen size, flowing in the page — never
// overlapping the text. `image` is an `att:<turn>/<file>` attachment ref (or a
// template-relative asset path); `image_size` picks the width; `image_position`
// puts it above or below the body.
#let doc-image = inputs.at("image", default: "")
#let img-width = (
  small: 40%, medium: 65%, large: 85%, full: 100%,
).at(inputs.at("image_size", default: "large"), default: 85%)
#let img-caption = inputs.at("image_caption", default: "")
#let img-pos = inputs.at("image_position", default: "bottom")
