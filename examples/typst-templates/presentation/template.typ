// Example corporate presentation — 16:9 slide deck, neutral sample branding.
// This is the render entrypoint. Brand tokens + deck data live in
// `context.typ`; every slide layout lives in `layouts.typ` (shared,
// byte-identical across the sample and croit-branded template sets).
//
// CONTENT IS FULLY SEPARATE: this template is never edited. The model only
// writes a JSON deck (default `deck.json`) describing the slides; this renderer
// turns it into branded slides. Compile with:
//   typst compile template.typ deck.pdf --font-path fonts --input data=deck.json
//
// Each slide object has a `layout` (cover|agenda|section|content|split|cards|
// stats|hbar|bar|line|donut|matrix|… — the full catalogue lives in
// template.toml's description) plus the fields that layout needs. Brand
// (Urbanist, dark/white surfaces, the sample gradient, glass cards, 18pt
// radius, logo SVG) is applied automatically here. This sample ships no
// background art: cover/section/statement render on the plain themed
// background unless the deck sets a `bg_image`.

#import "context.typ": *
#import "layouts.typ": all-layouts

#set text(font: "Urbanist", fill: white)
#set page(width: SW, height: SH, margin: 0pt)

// Brand styling for prose written as native Typst markup inside slide bodies
// (content/split `body`+`bullets`, card bodies). The model writes plain Typst —
// *bold*, _italic_, `- `/`+ ` lists, #quote — and these rules colour Typst's own
// elements in the brand. They only affect eval'd body markup: the layouts build
// their titles/tables/bullets with text()/table()/grid(), not these elements.
#let body-heading-sizes = (20pt, 17pt, 15pt)
#show heading: it => {
  set text(fill: purple, weight: 600, size: body-heading-sizes.at(calc.min(it.level, 3) - 1))
  block(above: 8pt, below: 5pt, it.body)
}
#show link: it => text(fill: purple, underline(it))
#show quote.where(block: true): it => block(
  width: 100%, above: 6pt, below: 6pt, inset: (left: 12pt, top: 3pt, bottom: 3pt),
  stroke: (left: 3pt + purple), it.body,
)
#set enum(numbering: n => text(fill: purple, weight: 600)[#n.])
#set list(marker: (text(fill: purple)[▸], text(fill: purple)[‣], text(fill: purple)[·]))

#for (i, s) in slides.enumerate() {
  let n = i + 1
  let theme = s.at("theme", default: deck-theme)
  let p = palette(theme)
  let lay = s.at("layout", default: "content")
  page(fill: p.bg, all-layouts.at(lay, default: all-layouts.content)(p, n, s))
}
