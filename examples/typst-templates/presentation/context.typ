// Example presentation — brand context (neutral sample branding).
//
// This is the per-brand half of the presentation template: palette, geometry,
// palette-per-theme, background fallbacks and the deck data read from
// `sys.inputs`. It is imported by `layouts.typ` (the shared layout library,
// byte-identical across the sample and croit-branded template sets) and by
// `template.typ` (the render entrypoint). Replace the palette, logo and fonts
// below with your own, or point `[typst].templates_dir` at your own templates
// directory (see docs).
//
// CONTENT IS FULLY SEPARATE: the template is never edited. The model only
// writes a JSON data file (default `deck.json`) describing the slides; the
// renderer turns it into branded slides. Compile with:
//   typst compile template.typ deck.pdf --font-path fonts --input data=deck.json

// Content arrives one of two ways, both fully separate from this template:
//   • Gateway / tool use: `--input deck=<JSON string>` (the whole deck inline)
//   • Local dev:          `--input data=<path.json>` (defaults to deck.json)
#let _inline = sys.inputs.at("deck", default: none)
#let deck = if _inline != none { json(bytes(_inline)) } else { json(sys.inputs.at("data", default: "deck.json")) }
#let slides = deck.at("slides", default: ())
#let deck-title = deck.at("deck_title", default: "")
#let footer-left = deck.at("footer_left", default: "example.com")
// Optional deck-level footer meta: {author, date, confidential} — printed
// after footer-left on every slide's footer rule.
#let deck-meta = deck.at("meta", default: (:))
// Deck-wide theme keeps one consistent look; a slide may still override via
// its own `theme`, but the default avoids mixing dark + light in one deck.
// Precedence: --input theme=… overrides the deck's theme, which defaults dark.
#let deck-theme = sys.inputs.at("theme", default: deck.at("theme", default: "dark"))

// ---- Brand tokens (sample palette — swap for your own) ----------------------
#let anthracite = rgb("#1D1D1B")
#let purple = rgb("#2563EB")
#let peach = rgb("#0EA5E9")
#let dark-purple = rgb("#1E40AF")
#let teal = rgb("#0A889A")
#let lightgray = rgb("#F6F6F6")
#let mgray = rgb("#888888")
// Direction matches example.com: Purple → Peach, left to right (the CI gradient
// swatches list Purple first; the live site renders `to right, #2563EB → #0EA5E9`).
#let grad = gradient.linear(purple, peach, angle: 0deg)
#let grad-soft = gradient.linear(purple.transparentize(70%), peach.transparentize(70%), angle: 0deg)
// Exact site button gradient (#1E40AF → #0EA5E9, warmer than the CI accent).
#let cta-grad = gradient.linear(rgb("#1E40AF"), rgb("#0EA5E9"), angle: 0deg)
// Hex form of the accent for SVG icon recoloring (the `currentColor` swap).
#let accent-hex = "#2563EB"
// The white logo used over background photos / gradient panels.
#let logo-white = "assets/logo-light.svg"

// ---- Geometry (standard 16:9, 13.333in x 7.5in) ----------------------------
#let SW = 33.867cm
#let SH = 19.05cm
#let PADX = 1.7cm // body left/right padding
#let PADT = 2.9cm // body top (clears logo)
#let PADB = 1.7cm // body bottom (clears footer)
#let CW = SW - 2 * PADX // usable content width
// Usable content height. Needed explicitly because `height: 100%` inside `pad`
// resolves against the region BEFORE the bottom padding is taken off, so a
// full-height box would reach down through the footer. The extra clearance is
// because PADB alone does NOT clear the footer: the rule sits a hair *inside*
// the nominal content box, which only shows up when something is pinned to the
// very bottom of it (a caption under a full-height image was landing on the rule).
#let CH = SH - PADT - PADB - 0.55cm

// ---- Per-theme palette -----------------------------------------------------
#let palette(theme) = if theme == "light" {
  (
    bg: white, fg: anthracite, fg2: rgb("#4A4A4A"), muted: mgray,
    logo: "assets/logo-dark.svg",
    card-fill: white, card-line: rgb(0, 0, 0, 18),
    card-inner: white, zebra: lightgray, dark: false,
    icon-hex: "#1D1D1B",
  )
} else {
  (
    bg: anthracite, fg: white, fg2: rgb(255, 255, 255, 200), muted: mgray,
    logo: "assets/logo-light.svg",
    card-fill: rgb(255, 255, 255, 16), card-line: rgb(255, 255, 255, 46),
    card-inner: anthracite, zebra: rgb(255, 255, 255, 10), dark: true,
    icon-hex: "#FFFFFF",
  )
}

// This sample ships no background art, so every full-bleed layout falls back
// to the plain themed (or gradient) background. A deck can still set its own
// `bg_image`; the croit-branded set ships official background art here.
#let bg-fallbacks = (cover: "", section: "", statement: "", closing: "")
