// Example branded one-pager — neutral sample branding.
// This is the render entrypoint. Brand tokens + inputs live in `context.typ`;
// the typed block renderer lives in `blocks.typ` (shared, byte-identical across
// the sample and croit-branded template sets).
//
// A NON-letter branded document: title + optional subtitle + content, with a
// logo header and a generic page footer. Content is `body` (free-form native
// Typst markup) and/or `blocks` (a JSON array of typed blocks: section, text,
// bullets, stats, table, cards, image, quote, callout). `body` renders first,
// then the blocks.

#import "context.typ": *
#import "blocks.typ": render-block

// ---- Page (generic brand footer with page numbers) -------------------------
#set page(
  paper: "a4",
  margin: (left: 22mm, right: 22mm, top: 18mm, bottom: 26mm),
  footer: [
    #image("assets/brand-bar.png", width: 100%, height: 1pt)
    #v(3pt)
    #set text(size: 8pt, fill: muted)
    #set par(justify: false, leading: 0.5em)
    #grid(
      columns: (1fr, auto),
      align: (left + horizon, right + horizon),
      text(weight: "semibold", fill: c-primary)[© Example Corp],
      // `context` so `final()` can see the last page — "Page 1 of 3".
      context [Page #counter(page).display() of #counter(page).final().first()],
    )
  ],
  footer-descent: 4mm,
)
#set text(font: "Urbanist", size: 11pt, fill: ink, lang: lang)
#set par(justify: true, leading: 0.72em, spacing: 1.15em)

// ---- Brand styling for the body's native Typst elements --------------------
// The body is rendered with eval(mode: "markup"); these rules make Typst's own
// headings / links / quotes / lists / tables come out in the sample brand
// automatically, so the model just writes plain Typst markup and never sets
// colours itself.
#let heading-sizes = (15pt, 13pt, 11.5pt)
#show heading: it => {
  set text(fill: c-primary, weight: "bold", size: heading-sizes.at(calc.min(it.level, 3) - 1))
  block(above: if it.level == 1 { 4mm } else { 3mm }, below: 2mm, it.body)
}
#show link: it => underline(text(fill: c-primary, it))
#show quote.where(block: true): it => block(
  width: 100%, above: 8pt, below: 8pt,
  inset: (left: 12pt, top: 4pt, bottom: 4pt),
  stroke: (left: 3pt + c-primary),
  it.body,
)
#set enum(numbering: n => text(fill: c-primary, weight: 600)[#n.])
#set list(marker: (text(fill: c-primary)[•], text(fill: c-primary)[‣], text(fill: c-primary)[·]))
// Tables the model writes with #table(...) get the sample look (brand header
// row, light zebra body, hairline rules). Works with or without table.header.
#set table(
  inset: (x: 8pt, y: 5pt), align: left + horizon,
  stroke: (y: 0.5pt + rgb("#DCE4F7")),
  fill: (_, row) => if row == 0 { c-primary } else if calc.odd(row) { rgb("#EFF4FE") } else { white },
)
#show table.cell: it => if it.y == 0 {
  text(fill: white, weight: "semibold", it)
} else { it }

// ---- Optional content image ------------------------------------------------
#let render-doc-image() = {
  if doc-image != "" {
    v(6mm)
    align(center, image(doc-image, width: img-width))
    if img-caption != "" {
      v(2mm)
      align(center, text(size: 9pt, fill: muted, style: "italic")[#img-caption])
    }
  }
}

// ---- Header: logo + brand gradient rule ------------------------------------
#image("assets/logo.svg", height: 10mm)
#v(6pt)
#image("assets/brand-bar.png", width: 100%, height: 2pt)

#v(9mm)

// ---- Title + optional subtitle ---------------------------------------------
#text(weight: "bold", size: 22pt, fill: ink)[#title]
#if subtitle != "" {
  v(2mm)
  text(weight: "medium", size: 13pt, fill: c-primary)[#subtitle]
}

#v(7mm)

// Content image above the body when requested.
#if img-pos == "top" { render-doc-image(); v(7mm) }

// ---- Body: native Typst markup (the original mechanism) ---------------------
#if body != "" { eval(body, mode: "markup") }

// ---- Blocks: the typed block system -----------------------------------------
// Rendered after the body; each block flows vertically in the page.
#if blocks.len() > 0 {
  v(4mm)
  for (i, b) in blocks.enumerate() {
    render-block(b)
    if i < blocks.len() - 1 { v(3.5mm) }
  }
}

// Content image below the body (the default position).
#if img-pos != "top" { render-doc-image() }
