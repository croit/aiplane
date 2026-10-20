// ============================================================================
// SHARED BLOCK RENDERER — one-pager
//
// This file is byte-identical between the sample templates dir and the
// croit-branded dir. It imports the brand tokens from `context.typ` (the
// per-brand file) and renders the typed `blocks` system. The block types give
// the model a way to compose a richer one-pager than free-form `body` markup:
// section, text, bullets, stats, table, cards, image, quote, callout.
//
// Every block is compact and flows vertically in the A4 page — never fixed to a
// slide size. Brand colours come from context.typ, so no colour is hardcoded
// here.
// ============================================================================

#import "context.typ": (c-primary, c-accent, ink, muted, brand)

// light brand tint for soft backgrounds (stat cells, callouts, cards)
#let tint = c-primary.transparentize(92%)
#let line = c-primary.transparentize(78%)

// ---- section: a heading + optional body text --------------------------------
#let bl-section(b) = {
  let h = b.at("heading", default: "")
  if h != "" {
    text(weight: "bold", size: 14pt, fill: c-primary)[#h]
    v(1.5mm)
  }
  let body = b.at("body", default: "")
  if body != "" { eval(body, mode: "markup") }
}

// ---- text: a paragraph of native Typst markup -------------------------------
#let bl-text(b) = eval(b.at("content", default: ""), mode: "markup")

// ---- bullets: a branded bulleted list ---------------------------------------
#let bl-bullets(b) = list(
  marker: text(fill: c-primary)[•],
  ..b.at("items", default: ()).map(it => eval(it, mode: "markup")),
)

// ---- stats: a horizontal row of big-number callouts -------------------------
#let bl-stats(b) = {
  let st = b.at("stats", default: ())
  grid(
    columns: st.map(_ => 1fr), column-gutter: 4mm,
    ..st.map(x => {
      let note = x.at("note", default: "")
      block(
        radius: 3mm, fill: tint, stroke: 0.5pt + line,
        inset: (x: 4mm, y: 3mm),
        stack(
          text(weight: "bold", size: 17pt, fill: c-primary)[#x.at("value", default: "")],
          block(height: 1mm),
          text(size: 9pt, weight: "medium", fill: ink)[#x.at("label", default: "")],
          if note != "" { text(size: 8pt, fill: muted)[#note] } else { [] },
        ),
      )
    }))
}

// ---- table: a branded data table (styled by the #set table rule) ------------
#let bl-table(b) = {
  let cols = b.at("columns", default: ())
  let rows = b.at("rows", default: ())
  table(
    columns: (1fr,) * calc.max(cols.len(), 1),
    table.header(..cols.map(c => text(weight: "semibold", size: 10pt)[#c])),
    ..rows.flatten().map(c => text(size: 10pt, fill: ink)[#c]),
  )
}

// ---- cards: a horizontal row of small titled cards --------------------------
#let bl-cards(b) = {
  let cs = b.at("cards", default: ())
  grid(
    columns: cs.map(_ => 1fr), column-gutter: 4mm,
    ..cs.map(c => block(
      radius: 3mm, stroke: 0.5pt + line, fill: white,
      inset: (x: 4mm, y: 3mm),
      stack(
        text(weight: "bold", size: 11pt, fill: ink)[#c.at("title", default: "")],
        block(height: 1.5mm),
        text(size: 9.5pt, fill: muted, eval(c.at("body", default: ""), mode: "markup")),
      ),
    )))
}

// ---- image: a centred picture with optional caption -------------------------
#let bl-image(b) = {
  let src = b.at("src", default: "")
  if src != "" {
    let w = (small: 40%, medium: 65%, large: 85%, full: 100%)
      .at(b.at("size", default: "large"), default: 85%)
    align(center, image(src, width: w))
    let cap = b.at("caption", default: "")
    if cap != "" { v(1.5mm); align(center, text(size: 9pt, fill: muted, style: "italic")[#cap]) }
  }
}

// ---- quote: a pull quote with a brand left rule -----------------------------
#let bl-quote(b) = {
  let a = b.at("attribution", default: "")
  block(
    width: 100%, above: 2mm, below: 2mm,
    inset: (left: 10pt, top: 3pt, bottom: 3pt),
    stroke: (left: 3pt + c-primary),
    stack(
      text(size: 12pt, style: "italic", fill: ink)[#b.at("text", default: "")],
      block(height: 1mm),
      if a != "" { text(size: 9.5pt, fill: muted)[— #a] } else { [] },
    ),
  )
}

// ---- callout: a soft brand-tinted box with an optional label ----------------
#let bl-callout(b) = {
  let l = b.at("label", default: "")
  block(
    width: 100%, radius: 3mm, fill: tint, stroke: 0.5pt + line,
    inset: (x: 4mm, y: 3mm), above: 2mm, below: 2mm,
    stack(
      if l != "" {
        text(size: 8.5pt, weight: "bold", tracking: 0.5pt, fill: c-primary)[#upper(l)]
        v(1mm)
      },
      text(size: 10.5pt, fill: ink, eval(b.at("text", default: ""), mode: "markup")),
    ),
  )
}

// ---- dispatch ----------------------------------------------------------------
#let render-block(b) = {
  let t = b.at("type", default: "text")
  if t == "section" { bl-section(b) }
  else if t == "text" { bl-text(b) }
  else if t == "bullets" { bl-bullets(b) }
  else if t == "stats" { bl-stats(b) }
  else if t == "table" { bl-table(b) }
  else if t == "cards" { bl-cards(b) }
  else if t == "image" { bl-image(b) }
  else if t == "quote" { bl-quote(b) }
  else if t == "callout" { bl-callout(b) }
  else { text(size: 9pt, fill: muted)[unknown block type: #t] }
}
