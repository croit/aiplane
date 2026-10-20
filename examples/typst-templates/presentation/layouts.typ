// ============================================================================
// SHARED LAYOUT LIBRARY — presentation
//
// This file is byte-identical between the sample templates dir and the
// croit-branded dir. It imports the brand tokens and deck data from
// `context.typ` (the per-brand file) and must reference nothing else from the
// outside. When you change it here, copy the result to the other set and keep
// them in sync (diff).
// ============================================================================

#import "context.typ": (
  anthracite, purple, peach, dark-purple, teal, lightgray, mgray,
  grad, grad-soft, cta-grad, accent-hex, logo-white,
  SW, SH, PADX, PADT, PADB, CW, CH,
  palette, bg-fallbacks, footer-left, deck-title, deck-meta,
)

// ---- Outline icons (Lucide-style) ------------------------------------------
// The `icon` field on a card names one of these; the SVG ships with
// stroke="currentColor", recoloured per theme here. Icons are white on dark /
// anthracite on light, outline-only.
#let icon-names = (
  "server", "activity", "trending-up", "shield", "database", "bell", "users",
  "zap", "cloud", "lock", "layers", "clock", "gauge", "globe", "rocket", "check-circle",
  "arrow-up-right",
)
#let lucide(name, hex, size) = box(height: size, baseline: 25%, image(
  bytes(read("assets/icons/" + name + ".svg").replace("currentColor", hex)),
  format: "svg", height: size,
))

// ---- Button system ----------------------------------------------------------
// Three styles, all 12pt radius + arrow:
//   primary  = gradient pill, white label        (use on dark backgrounds)
//   light    = white pill, accent label           (use on gradient panels)
//   outline  = transparent pill, white border/label (secondary, on gradient/dark)
#let btn-arrow(hex, size: 14pt) = box(baseline: 18%, image(
  bytes(read("assets/icons/arrow-up-right.svg").replace("currentColor", hex)), format: "svg", height: size))
#let btn(label, style: "primary") = {
  let inset = (x: 20pt, y: 10pt)
  let row(txtcol, arrowhex) = grid(columns: (auto, auto), column-gutter: 9pt, align: horizon,
    text(size: 14pt, weight: 500, fill: txtcol)[#label], btn-arrow(arrowhex))
  if style == "light" {
    box(fill: white, radius: 12pt, inset: inset, row(purple, accent-hex))
  } else if style == "outline" {
    box(stroke: 1pt + rgb(255, 255, 255, 150), radius: 12pt, inset: inset, row(white, "#FFFFFF"))
  } else {
    box(fill: cta-grad, radius: 12pt, inset: inset, row(white, "#FFFFFF"))
  }
}
// normalise `buttons` entries: a string → {label}, else the object as-is
#let norm-btns(arr) = arr.map(b => if type(b) == str { (label: b) } else { b })
#let button-row(arr, default-style: "primary") = {
  let bs = norm-btns(arr)
  grid(columns: bs.map(_ => auto), column-gutter: 12pt, align: horizon,
    ..bs.map(b => btn(b.at("label", default: ""), style: b.at("style", default: default-style))))
}
// single CTA (gradient pill)
#let cta-button(label) = btn(label, style: "primary")

// ---- Shared atoms -----------------------------------------------------------
#let logo-img(p, h: 0.62cm) = image(p.logo, height: h)
#let kicker(p, s) = {
  let k = s.at("kicker", default: "")
  if k != "" {
    align(right, text(size: 11pt, weight: 600, tracking: 1.5pt, fill: purple)[#upper(k)])
  }
}
#let title-block(p, s) = {
  set par(leading: 0.35em)
  text(size: 30pt, weight: 600, fill: p.fg)[#s.at("title", default: "")]
  let sub = s.at("subtitle", default: "")
  if sub != "" {
    v(2pt)
    text(size: 15pt, fill: p.fg2)[#sub]
  }
}
#let grad-text(body, size: 30pt, weight: 600) = text(
  size: size, weight: weight, fill: grad,
)[#body]

// glass card
#let glass(p, body, inset: 18pt) = block(
  fill: p.card-fill, stroke: 0.7pt + p.card-line, radius: 18pt,
  inset: inset, width: 100%, height: 100%,
)[#body]
// natural-height sibling for `glass`: identical styling minus `height: 100%`,
// for contexts that stack blocks at their content height (compose). A
// `height: 100%` inside an auto-height box still resolves against the page,
// so this is not wrappable.
#let glass-fit(p, body, inset: 18pt) = block(
  fill: p.card-fill, stroke: 0.7pt + p.card-line, radius: 18pt,
  inset: inset, width: 100%,
)[#body]
// gradient-border card (the "highlight")
#let grad-card(p, body, inset: 18pt) = block(
  fill: grad, radius: 18pt, inset: 1.2pt, width: 100%, height: 100%,
)[#block(fill: p.card-inner, radius: 17pt, inset: inset, width: 100%, height: 100%)[#body]]

// small pill badge / tag (glass chip)
#let badge(p, label) = box(fill: p.card-fill, stroke: 0.6pt + p.card-line, radius: 100pt,
  inset: (x: 11pt, y: 5pt), text(size: 10.5pt, weight: 600, fill: p.fg2)[#label])
#let badge-row(p, tags) = grid(columns: tags.map(_ => auto), column-gutter: 8pt,
  ..tags.map(t => badge(p, t)))

// ---- Image placement --------------------------------------------------------
// Cropping a picture to fill its box ("cover") is free for a photo but
// destructive for a diagram, chart or screenshot: a 4:3 diagram in a 16:9 hole
// loses a quarter of its width, and what goes missing is usually a label. So we
// only crop when the picture is already about as wide as the box it has to fill,
// and letterbox it ("contain") otherwise — nothing is ever cut off silently.
#let AR-TOL = 0.20
// natural aspect ratio of an image file (0.0 if unknown); needs a `context`,
// because `measure` does.
#let img-ar(path) = {
  let nat = measure(image(path))
  if nat.height == 0pt { 0.0 } else { nat.width / nat.height }
}
// May `path` be cropped into a box of this aspect without losing anything that
// matters? A 16:9 photo in a 16:9 hole: yes. A 4:3 diagram: no.
#let croppable(path, box-ar) = {
  let ar = img-ar(path)
  ar == 0.0 or calc.abs(ar - box-ar) <= box-ar * AR-TOL
}
// Draw `path` into a full-width, `h`-tall box, cropping only when that costs
// (almost) nothing. `layout` supplies the box width the aspect test needs.
#let fitted-image(path, h) = layout(sz => context box(
  width: 100%, height: h, clip: true,
  image(path, width: 100%, height: 100%,
    fit: if croppable(path, sz.width / h) { "cover" } else { "contain" }),
))

// browser/device frame around a screenshot (neutral window chrome). `h` = media
// height; the frame adds a slim title bar with three muted dots above the image.
#let device-frame(img, h: 11cm) = block(radius: 12pt, clip: true, width: 100%,
  stroke: 0.8pt + rgb(255, 255, 255, 38), fill: rgb("#16171B"))[
  #block(width: 100%, fill: rgb("#202126"), inset: (x: 12pt, y: 8pt))[
    #grid(columns: (auto, auto, auto), column-gutter: 7pt,
      ..((rgb("#5A5A60"),) * 3).map(c => circle(radius: 3.5pt, fill: c, stroke: none)))
  ]
  #fitted-image(img, h)
]

// ---- Backgrounds ------------------------------------------------------------
// full-bleed background image + anthracite scrim (keeps text legible). Takes a
// resolved path: "" or "none" → nothing placed.
#let resolve-bg(s, fallback) = {
  let bg = s.at("bg_image", default: fallback)
  if bg == "none" { "" } else { bg }
}
#let bg-scrim(bg, scrim: 40%) = {
  if bg != "" {
    place(top + left, image(bg, width: SW, height: SH, fit: "cover"))
    place(top + left, rect(width: SW, height: SH, fill: anthracite.transparentize(scrim)))
  }
}

// ---- People / number badges -------------------------------------------------
// circular image avatar, or gradient initials when no avatar is given
#let avatar(p, person, d: 2.6cm) = {
  let av = person.at("avatar", default: "")
  if av != "" {
    box(width: d, height: d, block(radius: 50%, clip: true, width: 100%, height: 100%,
      image(av, width: 100%, height: 100%, fit: "cover")))
  } else {
    let initials = person.at("name", default: "?").split(" ").map(w => w.slice(0, 1)).join("")
    box(width: d, height: d, block(fill: grad, radius: 50%, inset: 0pt, width: 100%, height: 100%,
      align(center + horizon, text(size: 22pt, weight: 600, fill: white)[#initials])))
  }
}

// true circular number badge (gradient ring, inner fill, number)
#let num-badge(p, label, d: 1.3cm) = box(width: d, height: d, block(
  fill: grad, radius: 50%, inset: 1.4pt, width: 100%, height: 100%,
)[#block(fill: p.card-inner, radius: 50%, width: 100%, height: 100%, inset: 0pt,
    align(center + horizon, text(size: 15pt, weight: 600, fill: p.fg)[#label]))])

// ---- Footer / chrome --------------------------------------------------------
// the one canonical footer, shared by every slide for consistency. Optional
// deck-level `meta` (author / date / confidential) prints after footer-left.
#let footer-row(p, n, on-grad: false) = {
  let txtcol = if on-grad { rgb(255, 255, 255, 220) } else { p.muted }
  // A gradient *fill* on a thin rect, not a gradient *stroke* on a line:
  // typ2pptx renders DrawingML gradient fills natively but drops gradient
  // line strokes, so the footer rule vanished on some exported slides.
  let rule-fill = if on-grad { rgb(255, 255, 255, 170) } else { grad }
  let meta = (
    deck-meta.at("author", default: ""),
    deck-meta.at("date", default: ""),
    if deck-meta.at("confidential", default: false) == true { "Confidential" } else { "" },
  ).filter(x => x != "")
  place(bottom + left, dx: PADX, dy: -0.85cm, box(width: CW)[
    #rect(width: 100%, height: 1pt, fill: rule-fill, stroke: none)
    #v(5pt)
    #grid(
      columns: (1fr, auto),
      text(size: 9pt, fill: txtcol)[
        #footer-left
        #if meta.len() > 0 [ \ · #meta.join(" · ")]
      ],
      text(size: 9pt, fill: txtcol)[#deck-title #h(6pt) · #h(6pt) #{ if n < 10 { "0" } }#str(n)],
    )
  ])
}

// the signature bottom band: one line of gradient "takeaway" every content
// slide may carry. Reserved INSIDE the content box (never over the footer),
// so the body above it gets the remaining height and cannot overlap it.
#let has-takeaway(s) = s.at("takeaway", default: "") != ""
#let takeaway-band(p, s) = block(
  width: 100%, fill: grad, radius: 10pt, stroke: none, inset: (x: 14pt, y: 8pt),
  grid(columns: (auto, 1fr), column-gutter: 12pt, align: horizon,
    text(size: 10pt, weight: 600, tracking: 1.2pt, fill: rgb(255, 255, 255, 90%))[
      #upper(s.at("takeaway_label", default: "Takeaway"))],
    text(size: 13pt, weight: 500, fill: white)[#s.at("takeaway", default: "")]))

// chrome wrapper for standard content slides (logo + kicker + footer + body),
// with the optional takeaway band reserved at the bottom of the content box.
#let chrome(p, n, s, body) = {
  place(top + left, dx: PADX, dy: 1.15cm, logo-img(p))
  place(top + right, dx: -PADX, dy: 1.3cm, box(width: 14cm, kicker(p, s)))
  footer-row(p, n)
  if has-takeaway(s) {
    pad(left: PADX, right: PADX, top: PADT, bottom: PADB,
      box(height: CH, grid(rows: (1fr, auto), row-gutter: 0.45cm, body, takeaway-band(p, s))))
  } else {
    pad(left: PADX, right: PADX, top: PADT, bottom: PADB, body)
  }
}

// ---- Metric atoms (the `dashboard` layout) ----------------------------------
// A signed delta chip, coloured by direction: "+…" reads as up (green), "-…"
// (ASCII or the − minus sign) as down (peach), anything else muted.
#let trend-chip(p, t) = {
  if t == "" { return [] }
  let up = t.starts-with("+")
  let down = t.starts-with("-") or t.starts-with("−")
  let col = if up { rgb("#38A76F") } else if down { peach } else { p.muted }
  text(size: 12.5pt, weight: 600, fill: col)[#t]
}
// A mini trend line for a metric card. Solid-colour stroke (NOT a gradient
// stroke — typ2pptx drops those), so it survives the editable-pptx export.
#let sparkline(data, h: 1.15cm, paint: purple) = {
  if type(data) != array or data.len() < 2 { return [] }
  let lo = calc.min(..data)
  let hi = calc.max(..data)
  let span = calc.max(hi - lo, 0.0001)
  layout(size => {
    let ww = size.width
    let n = data.len()
    let pts = data.enumerate().map(iv => (
      ww * (iv.at(0) / (n - 1)),
      h * (1 - (iv.at(1) - lo) / span),
    ))
    box(width: 100%, height: h, curve(
      stroke: 1.6pt + paint,
      curve.move(pts.first()),
      ..pts.slice(1).map(pt => curve.line(pt)),
    ))
  })
}

// ---- Bullet list atom (content / split / findings / compose share) ----------
#let bullet-list(p, items, size: 15pt) = {
  set par(leading: 0.6em)
  for b in items {
    grid(
      columns: (auto, 1fr), column-gutter: 10pt,
      text(size: size, fill: purple)[▸],
      text(size: size, fill: p.fg, eval(b, mode: "markup")),
    )
    v(7pt)
  }
}

// ---- Feature-card atom (cards + compose share) ------------------------------
#let feature-card(p, c, style: "glass", h: 8.7cm) = {
  let ic = c.at("icon", default: "")
  let im = c.at("image", default: "")
  let icon-cell = if ic == "" { [] } else if icon-names.contains(ic) {
    lucide(ic, p.icon-hex, 22pt)
  } else { text(size: 19pt, fill: p.fg)[#ic] }
  let head = grid(
    columns: (auto, 1fr), column-gutter: 10pt, align: horizon,
    icon-cell,
    text(size: 17pt, weight: 600, fill: p.fg)[#c.at("title", default: "")],
  )
  let body = text(size: 12.5pt, fill: p.fg2, eval(c.at("body", default: ""), mode: "markup"))
  // optional inline "learn more ↗" link (product-card style)
  let lk = c.at("link", default: "")
  let link-line = if lk == "" { [] } else {
    v(10pt)
    grid(columns: (auto, auto), column-gutter: 7pt, align: horizon,
      text(size: 12.5pt, weight: 600, fill: purple)[#lk], btn-arrow(accent-hex, size: 13pt))
  }
  box(height: h, {
    if im != "" {
      // image banner on top, content below — card clips to 18px radius
      block(fill: p.card-fill, stroke: 0.7pt + p.card-line, radius: 18pt, clip: true, width: 100%, height: 100%)[
        #image(im, width: 100%, height: 3.4cm, fit: "cover")
        #pad(x: 16pt, y: 16pt)[#head #v(8pt) #body #link-line]
      ]
    } else if style == "gradient" {
      grad-card(p, { head; v(10pt); body; link-line })
    } else {
      glass(p, { head; v(10pt); body; link-line })
    }
  })
}

// ---- Process row atom (process + compose share) -----------------------------
#let process-row(p, steps) = {
  let cnt = calc.max(steps.len(), 1)
  // band: connector line runs through the vertical centre of the badges
  block(width: 100%, height: 1.3cm, {
    place(left + horizon, line(length: 100%, stroke: 2.5pt + grad))
    grid(
      columns: (1fr,) * cnt, align: center + horizon,
      ..steps.enumerate().map(((i, st)) => num-badge(p, str(i + 1)))
    )
  })
  v(14pt)
  grid(
    columns: (1fr,) * cnt, align: center,
    ..steps.map(st => {
      text(size: 15pt, weight: 600, fill: p.fg)[#st.at("label", default: "")]
      let d = st.at("detail", default: "")
      if d != "" { v(4pt); linebreak(); text(size: 12pt, fill: p.muted)[#d] }
    })
  )
}

// ---- Data table atom (table / matrix / compose share) -----------------------
// A cell that is exactly a mark glyph is rendered in the brand's mark colours
// (✓ green, ✕/✗/× peach, – muted); every other cell prints as plain text.
#let mark-glyph(p, txt) = {
  if txt == "✓" { text(size: 13pt, weight: 600, fill: rgb("#38A76F"))[✓] }
  else if txt == "✕" or txt == "✗" or txt == "×" { text(size: 13pt, weight: 600, fill: peach)[#txt] }
  else if txt == "–" { text(size: 12.5pt, fill: p.muted)[–] }
  else { text(size: 12.5pt, fill: p.fg)[#txt] }
}
#let data-table(p, cols, rows, dense: false, first_col: false, caption: "") = {
  let ncol = calc.max(cols.len(), 1)
  let inset = if dense { 7pt } else { 11pt }
  let tsize = if dense { 11.5pt } else { 13pt }
  table(
    columns: (1fr,) * ncol,
    stroke: none,
    inset: inset,
    fill: (col, row) => if row == 0 { purple } else if calc.even(row) { p.zebra } else { none },
    table.header(..cols.map(c => text(size: tsize, weight: 600, fill: white)[#c])),
    ..rows.flatten().enumerate().map(((i, cell)) => {
      let cidx = calc.rem(i, ncol)
      if first_col and cidx == 0 { text(size: tsize, weight: 600, fill: p.fg)[#cell] }
      else { mark-glyph(p, cell) }
    }),
  )
  if caption != "" { v(5pt); text(size: 11pt, fill: p.muted)[#caption] }
}

// ---- Chart atoms (native vector, brand series colours) ----------------------
// Series colours cycle through the brand hues already in the token set, so a
// chart never introduces a new colour. All charts are solid-stroke / solid-
// fill vector, so they stay crisp and survive the editable-pptx export.
#let series-color(i) = (purple, peach, teal, dark-purple).at(calc.rem(i, 4))
#let max-of(vals) = if vals.len() == 0 { 0 } else { calc.max(..vals) }
#let num-fmt(v) = {
  let neg = v < 0
  let x = calc.abs(v)
  let r = calc.round(x)
  let body = if x == r {
    let ip = str(r)
    let n = ip.len()
    let out = ""
    for i in range(n) {
      out = out + ip.at(i)
      if calc.rem(n - i - 1, 3) == 0 and i != n - 1 {
        out = out + ","
      }
    }
    out
  } else {
    str(calc.round(x * 10) / 10)
  }
  if neg { "-" + body } else { body }
}
#let chart-legend(p, series, size: 10.5pt) = grid(
  columns: series.map(_ => auto), column-gutter: 16pt, align: horizon,
  ..series.enumerate().map(iv => grid(columns: (auto, auto), column-gutter: 6pt, align: horizon,
    rect(width: 0.32cm, height: 0.32cm, fill: series-color(iv.at(0)), radius: 2pt, stroke: none),
    text(size: size, fill: p.fg2)[#iv.at(1).at("name", default: "")])))
// small gradient badge for chart callouts (e.g. "+13%")
#let chip-badge(txt) = box(fill: grad, radius: 5pt, stroke: none,
  inset: (x: 8pt, y: 2.5pt), text(size: 10pt, weight: 600, fill: white)[#txt])

// Horizontal grouped bars — the benchmark-deck workhorse: long category labels
// on the left, one bar per series per category, value labels at the bar end.
#let hbar-chart(p, categories, series, h: 6.4cm, labels: true) = {
  let n = calc.max(categories.len(), 1)
  let ns = calc.max(series.len(), 1)
  let maxv = max-of(series.map(s => max-of(s.at("values", default: ()))))
  if maxv <= 0 { maxv = 1 }
  let gutter = 0.4cm
  let row-h = (h - (n - 1) * gutter) / n
  let bar-h = (row-h - (ns - 1) * 2.5pt) / ns
  let cells = ()
  for (i, cat) in categories.enumerate() {
    cells.push(align(right + horizon, text(size: 11pt, weight: 500, fill: p.fg2)[#cat]))
    let bars = ()
    for (si, s) in series.enumerate() {
      let v = s.at("values").at(i, default: 0)
      let col = series-color(si)
      let w = if v > 0 { calc.max(v / maxv, 0.005) * 100% } else { 0% }
      bars.push(grid(columns: (1fr, auto), column-gutter: 6pt,
        box(width: 100%, height: 100%,
          box(width: w, height: 100%, fill: col, radius: 2pt, stroke: none)),
        if labels { text(size: 9.5pt, weight: 600, fill: p.fg2)[#num-fmt(v)] } else { [] }))
    }
    cells.push(align(left + horizon, grid(rows: (bar-h,) * ns, row-gutter: 2.5pt, ..bars)))
  }
  grid(
    columns: (5.5cm, 1fr), column-gutter: 10pt,
    rows: (row-h,) * n, row-gutter: gutter,
    ..cells)
  v(8pt)
  chart-legend(p, series)
}

// Vertical grouped bars with per-bar value labels and optional per-category
// gradient callout chips (e.g. "+13%").
#let bar-chart(p, categories, series, h: 6.4cm, callouts: (), labels: true) = {
  let n = calc.max(categories.len(), 1)
  let maxv = max-of(series.map(s => max-of(s.at("values", default: ()))))
  if maxv <= 0 { maxv = 1 }
  let plot-h = h - 1.0cm
  let bar-h-max = plot-h - 0.6cm
  let bar-w = calc.min(1.1cm, (CW / n) * 0.18)
  // one bar column (value label above the bar, bottom-anchored)
  let bar-col(v, col) = {
    let bh = if v > 0 { calc.max(v / maxv * bar-h-max, 2pt) } else { 0pt }
    if labels and v > 0 {
      stack(spacing: 2pt,
        text(size: 9pt, weight: 600, fill: p.fg2)[#num-fmt(v)],
        box(height: bh, width: 100%, fill: col, radius: 2pt, stroke: none))
    } else {
      box(height: bh, width: 100%, fill: col, radius: 2pt, stroke: none)
    }
  }
  // one category group: optional callout chip, bar row, category label
  let cat-group(i, cat) = {
    let callout = callouts.at(i, default: "")
    let bar-row = align(center + bottom,
      grid(columns: series.map(_ => auto), column-gutter: 3pt, align: bottom,
        ..series.enumerate().map(ss =>
          box(width: bar-w, block(height: 100%,
            align(bottom, bar-col(
              ss.at(1).at("values").at(i, default: 0), series-color(ss.at(0)))))))))
    box(width: 100%,
      grid(rows: (auto, 1fr, auto), row-gutter: 4pt,
        if callout == "" { [] } else { align(center, chip-badge(callout)) },
        bar-row,
        align(center, text(size: 10pt, weight: 500, fill: p.fg2)[#cat])))
  }
  layout(size => grid(
    columns: (1fr,) * n, column-gutter: 0.4cm,
    ..categories.enumerate().map(((i, cat)) => cat-group(i, cat))))
  v(6pt)
  chart-legend(p, series)
}


// Line chart, 1–3 series with markers, two gridlines, x labels, legend.
#let line-chart(p, categories, series, h: 6.4cm) = {
  let n = calc.max(categories.len(), 1)
  let maxv = max-of(series.map(s => max-of(s.at("values", default: ())))) * 1.12
  if maxv <= 0 { maxv = 1 }
  let plot-h = h - 0.9cm
  box(width: 100%,
    grid(rows: (1fr, auto, auto), row-gutter: 5pt,
      layout(size => {
        let ww = size.width
        let pts-of(vals) = vals.enumerate().map(iv => (
          if n > 1 { ww * iv.at(0) / (n - 1) } else { ww / 2 },
          plot-h * (1 - iv.at(1) / maxv)))
        box(width: 100%, height: plot-h, {
          place(bottom + left, line(length: 100%, stroke: 0.7pt + p.card-line))
          place(top + left, dy: plot-h / 2, line(length: 100%, stroke: 0.4pt + p.card-line))
          for (i, s) in series.enumerate() {
            let col = series-color(i)
            let pts = pts-of(s.at("values", default: ()))
            if pts.len() > 1 {
              curve(stroke: 1.7pt + col, curve.move(pts.first()), ..pts.slice(1).map(pt => curve.line(pt)))
            }
            for pt in pts {
              place(top + left, dx: pt.at(0) - 2.6pt, dy: pt.at(1) - 2.6pt,
                circle(radius: 2.6pt, fill: col, stroke: 0.9pt + p.bg))
            }
          }
        })
      }),
      align(center,
        grid(columns: (1fr,) * n,
          ..categories.map(c => text(size: 10pt, weight: 500, fill: p.fg2)[#c]))),
      chart-legend(p, series)))
}

// Donut (default) or pie: filled wedges in brand series colours + a legend
// with values and percentages. `center_label` prints in the donut hole.
// Points along a circular arc (Typst has no native arc path command, so arcs
// are approximated with fine line segments — 48 per full turn is smooth at print).
// `cx`/`cy` shift the circle's center so it sits inside the host box.
#let arc-pts(radius, from, to, cx: 0pt, cy: 0pt) = {
  let span = (to - from) / 1deg
  let steps = calc.round(calc.abs(span) / 360 * 48)
  if steps < 2 { steps = 2.0 }
  let pts = ()
  let k = 0
  while k <= steps {
    let t = from + (to - from) * (k / steps)
    pts.push((cx + radius * calc.cos(t), cy + radius * calc.sin(t)))
    k = k + 1
  }
  pts
}

#let donut-chart(p, slices, style: "donut", center_label: "") = {
  let vals = slices.map(s => s.at("value", default: 0))
  let total = 0
  for x in vals { total = total + x }
  if total <= 0 { total = 1 }
  let R = 2.6cm
  let r0 = if style == "donut" { 1.55cm } else { 0cm }
  let acc = 0.0
  let wedges = ()
  for i in range(slices.len()) {
    let v = vals.at(i)
    let a0 = acc / total * 360deg
    let a1 = (acc + v) / total * 360deg
    acc += v
    let col = series-color(i)
    let outer = arc-pts(R, a0, a1, cx: R, cy: R)
    if r0 > 0cm {
      let inner = arc-pts(r0, a1, a0, cx: R, cy: R)
      wedges.push(curve(
        curve.move(outer.at(0)),
        ..outer.slice(1).map(pt => curve.line(pt)),
        ..inner.slice(1).map(pt => curve.line(pt)),
        curve.close(),
        fill: col, stroke: 1pt + p.bg))
    } else {
      wedges.push(curve(
        curve.move((R, R)),
        curve.line(outer.at(0)),
        ..outer.slice(1).map(pt => curve.line(pt)),
        curve.close(),
        fill: col, stroke: 1pt + p.bg))
    }
  }
  let legend = ()
  for (i, s) in slices.enumerate() {
    let v = s.at("value", default: 0)
    legend.push(rect(width: 0.32cm, height: 0.32cm, fill: series-color(i), radius: 2pt, stroke: none))
    legend.push(text(size: 11.5pt, weight: 500, fill: p.fg)[#s.at("name", default: "")])
    legend.push(text(size: 11.5pt, fill: p.fg2)[
      #num-fmt(v) #h(6pt) #text(fill: p.muted)[(#calc.round(v / total * 100)%)]])
  }
  grid(columns: (auto, 1fr), column-gutter: 1.6cm, align: horizon,
    box(width: 2 * R, height: 2 * R, {
      for w in wedges { place(top + left, dx: 0pt, dy: 0pt, w) }
      if center_label != "" {
        place(center, text(size: 15pt, weight: 600, fill: p.fg)[#center_label])
      }
    }),
    grid(columns: (auto, auto, 1fr), column-gutter: 8pt, row-gutter: 6pt, ..legend))
}

// ============================================================================
// LAYOUTS
// ============================================================================

#let l-cover(p, s) = {
  let bg = resolve-bg(s, bg-fallbacks.cover)
  let on-photo = bg != ""
  bg-scrim(bg, scrim: 32%)
  place(top + left, dx: PADX, dy: 1.3cm, logo-img(if on-photo { (logo: logo-white) } else { p }, h: 0.7cm))
  // Over a background photo the title is solid WHITE; on a plain background
  // it uses the brand gradient.
  let title-it(t, size: 48pt) = if on-photo { text(size: size, weight: 600, fill: white)[#t] } else { grad-text(t, size: size) }
  pad(left: PADX, right: PADX, top: 6.0cm, bottom: PADB, {
    let eb = s.at("eyebrow", default: "")
    if eb != "" {
      text(size: 12pt, weight: 600, tracking: 2pt, fill: if on-photo { rgb(255, 255, 255, 235) } else { purple })[#upper(eb)]
      v(10pt)
    }
    // explicit per-line blocks with a guaranteed gap — display type at this
    // size has deep descenders (g, comma) that overlap the next line's
    // ascenders if we lean on paragraph leading alone.
    set par(leading: 0.3em)
    block(below: 30pt, title-it(s.at("title", default: "")))
    let t2 = s.at("title_line_2", default: "")
    if t2 != "" { block(title-it(t2)) }
    let sub = s.at("subtitle", default: "")
    if sub != "" {
      v(14pt)
      text(size: 18pt, fill: if on-photo { rgb(255, 255, 255, 230) } else { p.fg2 })[#sub]
    }
  })
  place(bottom + left, dx: PADX, dy: -1.2cm, box(width: CW, grid(
    columns: (1fr, auto),
    text(size: 12pt, fill: p.fg)[
      #let nm = s.at("presenter", default: "")
      #let rl = s.at("role", default: "")
      #if nm != "" [#text(weight: 600)[#nm]#if rl != "" [ · #text(fill: p.muted)[#rl]]]
    ],
    text(size: 12pt, fill: p.muted)[#s.at("date", default: "")],
  )))
}

#let l-section(p, n, s) = {
  let bg = resolve-bg(s, bg-fallbacks.section)
  let on-photo = bg != ""
  if on-photo { bg-scrim(bg, scrim: 42%) } else { place(top + left, rect(width: SW, height: SH, fill: grad-soft)) }
  place(top + left, dx: PADX, dy: 1.3cm, logo-img(if on-photo { (logo: logo-white) } else { p }, h: 0.7cm))
  let tcol = if on-photo { white } else { p.fg }
  let scol = if on-photo { rgb(255, 255, 255, 230) } else { p.fg2 }
  pad(left: PADX, right: PADX, top: 6.5cm, {
    let num = s.at("number", default: "")
    if num != "" {
      text(size: 64pt, weight: 600, fill: if on-photo { rgb(255, 255, 255, 130) } else { p.fg2.transparentize(40%) })[#num]
      v(2pt)
    }
    text(size: 40pt, weight: 600, fill: tcol)[#s.at("title", default: "")]
    let sm = s.at("summary", default: "")
    if sm != "" { v(8pt); text(size: 16pt, fill: scol)[#sm] }
  })
  footer-row(p, n)
}

#let l-content(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  let tags = s.at("tags", default: ())
  if tags.len() > 0 { v(12pt); badge-row(p, tags) }
  v(14pt)
  let body = s.at("body", default: "")
  let bullets = s.at("bullets", default: ())
  if body != "" {
    set par(leading: 0.7em, spacing: 1.0em, justify: false)
    text(size: 15pt, fill: p.fg2, eval(body, mode: "markup"))
  }
  if bullets.len() > 0 { bullet-list(p, bullets, size: 15pt) }
})

#let l-agenda(p, n, s) = chrome(p, n, s, {
  text(size: 30pt, weight: 600, fill: p.fg)[#s.at("title", default: "Agenda")]
  v(18pt)
  let items = s.at("items", default: ())
  let rows = ()
  for (i, it) in items.enumerate() {
    let chip = num-badge(p, [#{ if i + 1 < 10 { "0" } }#str(i + 1)])
    rows.push((chip, it))
  }
  grid(
    columns: (1fr, 1fr), column-gutter: 1.2cm, row-gutter: 0.9cm,
    ..rows.map(((chip, it)) => grid(
      columns: (auto, 1fr, auto), column-gutter: 14pt, align: horizon,
      chip,
      [
        #text(size: 16pt, weight: 600, fill: p.fg)[#it.at("label", default: "")]
        #let note = it.at("note", default: "")
        #if note != "" [ \ #text(size: 12pt, fill: p.muted)[#note]]
      ],
      {
        let pg = it.at("page", default: "")
        if pg == "" { [] } else { text(size: 14pt, weight: 600, fill: purple)[#pg] }
      },
    ))
  )
})

#let l-cards(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(18pt)
  let cards = s.at("cards", default: ())
  grid(
    columns: (1fr,) * calc.max(cards.len(), 1), column-gutter: 0.7cm, rows: (8.7cm,),
    ..cards.map(c => feature-card(p, c, style: s.at("card_style", default: "glass")))
  )
})

#let l-split(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(16pt)
  let kind = s.at("right_kind", default: "text")
  let right = s.at("right", default: "")
  let right-block = if kind == "code" {
    glass(p, text(size: 11pt, font: "DejaVu Sans Mono", fill: p.fg)[#raw(right)])
  } else if kind == "image" and right != "" {
    if s.at("frame", default: false) {
      align(horizon, device-frame(right, h: 6.6cm))
    } else {
      block(radius: 18pt, clip: true, width: 100%, height: 100%, image(right, width: 100%, height: 100%, fit: "cover"))
    }
  } else {
    glass(p, text(size: 13.5pt, fill: p.fg2)[#right])
  }
  grid(
    columns: (1fr, 1fr), column-gutter: 0.9cm, rows: (7.8cm,),
    {
      let lb = s.at("left_body", default: "")
      if lb != "" { text(size: 14pt, fill: p.fg2, eval(lb, mode: "markup")); v(8pt) }
      bullet-list(p, s.at("left_bullets", default: ()), size: 14pt)
    },
    right-block,
  )
})

#let l-stats(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(0.9cm)
  let stats = s.at("stats", default: ())
  let per = calc.max(s.at("per_row", default: calc.min(stats.len(), 4)), 1)
  let rows = calc.ceil(stats.len() / per)
  grid(
    columns: (1fr,) * per,
    rows: ((CH - 4.2cm) / rows,) * rows,
    column-gutter: 0.6cm, row-gutter: 0.6cm,
    ..stats.map(st => align(center, {
      grad-text(st.at("value", default: ""), size: 50pt, weight: 600)
      v(6pt)
      text(size: 15pt, weight: 600, fill: p.fg)[#st.at("label", default: "")]
      let note = st.at("note", default: "")
      if note != "" { v(3pt); linebreak(); text(size: 11.5pt, fill: p.muted)[#note] }
      let sub = st.at("sub", default: "")
      if sub != "" { v(2pt); linebreak(); text(size: 10.5pt, fill: p.muted)[#sub] }
    }))
  )
})

// Metric dashboard: a row of glass cards, each a real metric — label, big
// gradient value, an optional signed trend, and an optional sparkline.
#let l-dashboard(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(0.8cm)
  let cards = s.at("cards", default: ())
  let cols = calc.max(cards.len(), 1)
  grid(
    columns: (1fr,) * cols, column-gutter: 0.6cm, rows: (6.4cm,),
    ..cards.enumerate().map(ic => {
      let c = ic.at(1)
      glass(p, {
        text(size: 11pt, weight: 600, tracking: 1.2pt, fill: p.muted)[#upper(c.at("label", default: ""))]
        v(10pt)
        grad-text(c.at("value", default: ""), size: 34pt, weight: 600)
        let tr = c.at("trend", default: "")
        if tr != "" { linebreak(); v(2pt); trend-chip(p, tr) }
        let sp = c.at("spark", default: ())
        if type(sp) == array and sp.len() >= 2 {
          place(bottom + left, dx: 0pt, dy: 0pt, box(width: 100%,
            sparkline(sp, paint: if calc.rem(ic.at(0), 2) == 0 { purple } else { peach })))
        }
      }, inset: 16pt)
    })
  )
})

#let l-comparison(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(16pt)
  let left = s.at("left", default: (:))
  let right = s.at("right", default: (:))
  let hl = s.at("highlight", default: "right")
  let col(data, win, mark, markcolor) = {
    let inner = {
      text(size: 16pt, weight: 600, fill: p.fg)[#data.at("heading", default: "")]
      v(10pt)
      for pt in data.at("points", default: ()) {
        grid(columns: (auto, 1fr), column-gutter: 9pt,
          text(size: 14pt, fill: markcolor)[#mark], text(size: 13.5pt, fill: p.fg2)[#pt])
        v(7pt)
      }
    }
    if win { grad-card(p, inner) } else { glass(p, inner) }
  }
  grid(
    columns: (1fr, 1fr), column-gutter: 0.9cm, rows: (7.6cm,),
    col(left, hl == "left", "×", peach),
    col(right, hl == "right", "✓", purple),
  )
})

#let l-process(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(1.2cm)
  process-row(p, s.at("steps", default: ()))
})

#let l-quote(p, n, s) = {
  let hero = s.at("style", default: "default") == "hero"
  let attr = [— #text(weight: 600)[#s.at("attribution", default: "")]#{
    let r = s.at("attribution_role", default: "")
    if r != "" [#text(fill: p.muted)[, #r]]
  }]
  if hero {
    place(top + left, dx: PADX, dy: 1.15cm, logo-img(p))
    place(top + right, dx: -PADX, dy: 1.3cm, box(width: 14cm, kicker(p, s)))
    place(center + horizon, box(width: 25cm, align(center, {
      set par(leading: 0.4em)
      text(size: 34pt, weight: 600, fill: p.fg)[“#s.at("quote", default: "")”]
      v(24pt)
      text(size: 15pt, fill: p.fg2)[#attr]
    })))
    footer-row(p, n)
  } else {
    chrome(p, n, s, {
      v(1.6cm)
      set par(leading: 0.5em)
      grad-text([“#s.at("quote", default: "")”], size: 30pt)
      v(22pt)
      text(size: 15pt, fill: p.fg)[#attr]
    })
  }
}

// `size` (small|medium|large|full) scales the picture as a FRACTION of the space
// that is actually free — the region `chrome` leaves between logo and footer —
// not as an absolute height. Fractions cannot overflow.
#let image-scale(s, default-key: "large") = (
  small: 0.45, medium: 0.68, large: 0.88, full: 1.0,
).at(s.at("size", default: default-key), default: 0.88)

// A single image on its own clean slide — centred, sized via `size`, NO browser
// chrome and NO text overlay. Optional `title` above and `caption` below sit in
// the flow, so nothing ever covers the image.
#let l-image(p, n, s) = chrome(p, n, s, {
  let img = s.at("image", default: "")
  let title = s.at("title", default: "")
  let cap = s.at("caption", default: "")
  let framed = s.at("frame", default: false)
  let scale = image-scale(s)
  box(width: 100%, height: CH, grid(
    columns: (1fr,), rows: (auto, 1fr, auto), row-gutter: 0.45cm,
    if title == "" { [] } else { align(center, text(size: 26pt, weight: 600, fill: p.fg)[#title]) },
    align(center + horizon, if img == "" { [] } else if framed {
      layout(sz => align(center, box(width: scale * 100%,
        device-frame(img, h: calc.max(sz.height * scale - 1.2cm, 1cm)))))
    } else {
      box(width: scale * 100%, height: scale * 100%,
        image(img, width: 100%, height: 100%, fit: "contain"))
    }),
    if cap == "" { [] } else { align(center, text(size: 13pt, fill: p.muted)[#cap]) },
  ))
})

// Image showcase. Text on a full-slide picture is the EXCEPTION, not the rule:
//   • no title and no caption → the picture alone, edge to edge, nothing on it
//   • title/caption, no `overlay_position` → the clean `image` slide
//   • `overlay_position` set → opt IN to the glass caption panel ON the picture
//   • `size` other than "full" → the clean sized `image` slide
//   • `frame: true` → screenshot inside the browser/device chrome
// A picture whose aspect is too far off 16:9 to crop is never full-bleed: it
// falls back to the clean slide rather than losing a quarter of itself.
#let l-media(p, s, n) = {
  let img = s.at("image", default: "")
  let framed = s.at("frame", default: false)
  let title = s.at("title", default: "")
  let cap = s.at("caption", default: "")
  let overlay-at = s.at("overlay_position", default: "")
  let has-text = title != "" or cap != ""
  let white-logo = (logo: logo-white, card-inner: anthracite, fg: white, muted: rgb(255, 255, 255, 220))
  if img != "" and framed {
    place(top + left, dx: PADX, dy: 1.15cm, logo-img(p))
    if title != "" { place(top + left, dx: PADX, dy: 2.7cm, text(size: 26pt, weight: 600, fill: p.fg)[#title]) }
    let h = (small: 6cm, medium: 8cm, large: 9.4cm, full: 11cm).at(s.at("size", default: "large"), default: 9.4cm)
    place(center + horizon, dy: -0.2cm, box(width: 21cm, device-frame(img, h: h)))
    if cap != "" { place(center + horizon, dy: 5.7cm, text(size: 13pt, fill: p.muted)[#cap]) }
    footer-row(p, n)
  } else if img != "" and (s.at("size", default: "full") != "full" or (has-text and overlay-at == "")) {
    l-image(p, n, (..s, size: s.at("size", default: "full")))
  } else if img != "" {
    context if not croppable(img, SW / SH) {
      l-image(p, n, (..s, size: "full"))
    } else {
      place(top + left, image(img, width: SW, height: SH, fit: "cover"))
      if has-text {
        let (anchor, dx, dy) = if overlay-at == "bottom-right" {
          (bottom + right, -PADX, -1.6cm)
        } else if overlay-at == "center" {
          (center + horizon, 0cm, 0cm)
        } else {
          (bottom + left, PADX, -1.6cm)
        }
        place(anchor, dx: dx, dy: dy, box(width: 18cm, block(
          fill: anthracite.transparentize(6%), stroke: 0.7pt + rgb(255, 255, 255, 60), radius: 18pt, inset: 18pt,
        )[
          #text(size: 22pt, weight: 600, fill: white)[#title]
          #if cap != "" { v(4pt); text(size: 13pt, fill: rgb(255, 255, 255, 220))[#cap] }
        ]))
      }
      place(top + left, dx: PADX, dy: 1.3cm, logo-img(white-logo, h: 0.7cm))
      footer-row(white-logo, n, on-grad: true)
    }
  } else {
    place(top + left, rect(width: SW, height: SH, fill: grad))
    place(top + left, dx: PADX, dy: 1.3cm, logo-img(white-logo, h: 0.7cm))
    place(center + horizon, box(width: 24cm, align(center, {
      text(size: 38pt, weight: 600, fill: white)[#title]
      if cap != "" { v(12pt); text(size: 17pt, fill: rgb(255, 255, 255, 230))[#cap] }
    })))
    footer-row(white-logo, n, on-grad: true)
  }
}

#let l-team(p, n, s) = chrome(p, n, s, {
  text(size: 30pt, weight: 600, fill: p.fg)[#s.at("title", default: "")]
  v(0.9cm)
  let ppl = s.at("people", default: ())
  grid(
    columns: (1fr,) * calc.max(ppl.len(), 1), column-gutter: 0.6cm,
    ..ppl.map(person => align(center, {
      avatar(p, person)
      v(10pt)
      text(size: 15pt, weight: 600, fill: p.fg)[#person.at("name", default: "")]
      v(2pt); linebreak()
      text(size: 12pt, fill: p.muted)[#person.at("role", default: "")]
    }))
  )
})

#let l-table(p, n, s) = chrome(p, n, s, {
  text(size: 30pt, weight: 600, fill: p.fg)[#s.at("title", default: "")]
  v(16pt)
  data-table(
    p, s.at("columns", default: ()), s.at("rows", default: ()),
    dense: s.at("dense", default: false),
    first_col: s.at("first_col", default: false),
    caption: s.at("caption", default: ""))
})

#let l-diagram(p, n, s) = chrome(p, n, s, {
  text(size: 30pt, weight: 600, fill: p.fg)[#s.at("title", default: "")]
  v(1.0cm)
  let nodes = s.at("nodes", default: ())
  // interleave nodes with gradient arrows
  let cells = ()
  let tracks = ()
  for (i, nd) in nodes.enumerate() {
    cells.push(box(width: 4.4cm, height: 2.4cm, glass(p, align(center + horizon, text(size: 14pt, weight: 600, fill: p.fg)[#nd.at("label", default: "")]), inset: 8pt)))
    tracks.push(auto)
    if i < nodes.len() - 1 {
      cells.push(align(horizon, text(size: 22pt, fill: purple)[→]))
      tracks.push(auto)
    }
  }
  align(center, grid(columns: tracks, column-gutter: 0.5cm, align: horizon, ..cells))
  let lg = s.at("legend", default: "")
  if lg != "" { v(0.8cm); align(center, text(size: 12pt, fill: p.muted)[#lg]) }
})

#let l-closing(p, n, s) = {
  let white-logo = (logo: logo-white)
  // dark background (like the cover) so the gradient CTA pops, exactly as the
  // website buttons do on their dark hero.
  let bg = resolve-bg(s, bg-fallbacks.closing)
  if bg != "" { bg-scrim(bg, scrim: 30%) } else { place(top + left, rect(width: SW, height: SH, fill: grad)) }
  place(top + left, dx: PADX, dy: 1.3cm, logo-img(white-logo, h: 0.7cm))
  pad(left: PADX, right: PADX, top: 5.4cm, {
    text(size: 44pt, weight: 600, fill: white)[#s.at("headline", default: "Let's build it together")]
    let sl = s.at("subline", default: "")
    if sl != "" { v(12pt); text(size: 18pt, fill: rgb(255, 255, 255, 220))[#sl] }
    let btns = s.at("buttons", default: ())
    let cta = s.at("cta_label", default: "")
    if btns.len() > 0 { v(22pt); button-row(btns, default-style: "primary") }
    else if cta != "" { v(22pt); cta-button(cta) }
    let parts = ()
    let nm = s.at("contact_name", default: "")
    let em = s.at("contact_email", default: "")
    if nm != "" { parts.push(nm) }
    if em != "" { parts.push(em) }
    if parts.len() > 0 {
      v(26pt)
      text(size: 13pt, fill: white)[#parts.join("   ·   ")]
    }
  })
  footer-row(white-logo, n, on-grad: true)
}

// big centred statement on a solid (theme) background
#let l-statement(p, n, s) = {
  let bg = resolve-bg(s, bg-fallbacks.statement)
  let on-photo = bg != ""
  bg-scrim(bg, scrim: 42%)
  place(top + left, dx: PADX, dy: 1.15cm, logo-img(if on-photo { (logo: logo-white) } else { p }))
  let tcol = if on-photo { white } else { p.fg }
  let scol = if on-photo { rgb(255, 255, 255, 230) } else { p.fg2 }
  place(center + horizon, box(width: 26cm, align(center, {
    let eb = s.at("eyebrow", default: "")
    if eb != "" { text(size: 12pt, weight: 600, tracking: 2pt, fill: if on-photo { rgb(255, 255, 255, 235) } else { purple })[#upper(eb)]; v(14pt) }
    text(size: 40pt, weight: 600, fill: tcol)[#s.at("title", default: "")]
    let sub = s.at("subtitle", default: "")
    if sub != "" { v(14pt); text(size: 18pt, fill: scol)[#sub] }
  })))
  footer-row(p, n)
}

// one hero metric + supporting copy
#let l-bignumber(p, n, s) = chrome(p, n, s, {
  grid(
    columns: (auto, 1fr), column-gutter: 1.4cm, align: horizon, rows: (7.5cm,),
    align(horizon, grad-text(s.at("value", default: ""), size: 110pt, weight: 600)),
    align(horizon, {
      text(size: 24pt, weight: 600, fill: p.fg)[#s.at("title", default: "")]
      let body = s.at("body", default: "")
      if body != "" { v(10pt); text(size: 15pt, fill: p.fg2)[#body] }
    }),
  )
})

// "trusted by" logo / name wall (glass chips)
#let l-logos(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(0.9cm)
  let names = s.at("logos", default: ())
  let per-row = s.at("per_row", default: 4)
  grid(
    columns: (1fr,) * per-row, column-gutter: 0.6cm, row-gutter: 0.6cm,
    ..names.map(nm => box(height: 2.2cm, glass(p, align(center + horizon, text(size: 17pt, weight: 600, fill: p.fg2)[#nm]))))
  )
})

// image gallery / grid (gradient-tile fallback when no image given)
#let l-gallery(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(0.8cm)
  let items = s.at("items", default: ())
  let per-row = s.at("per_row", default: 3)
  grid(
    columns: (1fr,) * per-row, column-gutter: 0.5cm, row-gutter: 0.5cm,
    ..items.map(it => {
      let img = it.at("image", default: "")
      let cap = it.at("caption", default: "")
      // solid caption strip BELOW the image (not overlaid) so text stays legible
      let strip = if p.dark { rgb("#26262A") } else { rgb("#ECECEE") }
      let cap-h = if cap != "" { 1.0cm } else { 0cm }
      let media = if img != "" { image(img, width: 100%, height: 100%, fit: "cover") } else { rect(width: 100%, height: 100%, fill: grad) }
      block(radius: 18pt, clip: true, width: 100%, height: 4.6cm, fill: strip,
        grid(
          rows: (1fr, cap-h), row-gutter: 0pt,
          box(width: 100%, height: 100%, clip: true, media),
          align(left + horizon, box(inset: (left: 12pt), text(size: 11.5pt, weight: 600, fill: p.fg)[#cap])),
        ))
    })
  )
})

// vertical roadmap / timeline
#let l-timeline(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(0.7cm)
  let ms = s.at("milestones", default: ())
  for (i, m) in ms.enumerate() {
    grid(
      columns: (auto, 1fr), column-gutter: 16pt, align: (center, left),
      {
        num-badge(p, str(i + 1), d: 1.0cm)
        if i < ms.len() - 1 { place(center, dx: 0pt, dy: 0pt, line(start: (0.5cm, 1.0cm), end: (0.5cm, 1.9cm), stroke: 2pt + grad)) }
      },
      {
        let dt = m.at("date", default: "")
        text(size: 16pt, weight: 600, fill: p.fg)[#m.at("label", default: "")]
        if dt != "" { h(10pt); text(size: 12pt, weight: 600, fill: purple)[#dt] }
        let d = m.at("detail", default: "")
        if d != "" { linebreak(); text(size: 13pt, fill: p.fg2)[#d] }
      },
    )
    v(0.45cm)
  }
})

// full-width gradient callout band on a dark slide. Buttons default to light+outline.
#let l-cta-panel(p, n, s) = {
  place(top + left, dx: PADX, dy: 1.15cm, logo-img(p))
  let m = 1.5cm
  place(center + horizon, box(width: SW - 2 * m, fill: grad, radius: 28pt, inset: (x: 2.2cm, y: 1.7cm), align(center, {
    let k = s.at("kicker", default: "")
    if k != "" { text(size: 12pt, weight: 600, tracking: 1.5pt, fill: rgb(255, 255, 255, 235))[#upper(k)]; v(12pt) }
    text(size: 32pt, weight: 600, fill: white)[#s.at("title", default: "")]
    let body = s.at("body", default: "")
    if body != "" { v(12pt); text(size: 16pt, fill: rgb(255, 255, 255, 235))[#body] }
    let btns = s.at("buttons", default: ())
    let cta = s.at("cta_label", default: "")
    if btns.len() > 0 { v(22pt); button-row(btns, default-style: "light") }
    else if cta != "" { v(22pt); btn(cta, style: "light") }
  })))
  footer-row(p, n)
}

// ============================================================================
// NEW LAYOUTS
// ============================================================================

// ---- Chart slides -----------------------------------------------------------
// title + a native-vector chart filling the content area + optional caption.
// The chart height is the free space minus title and caption, so a chart can
// never push its caption onto an extra slide.
#let chart-slide(p, n, s, chart) = chrome(p, n, s, {
  title-block(p, s)
  v(0.6cm)
  let cap = s.at("caption", default: "")
  let cap-h = if cap != "" { 0.5cm } else { 0cm }
  box(width: 100%, height: CH - 3.4cm - cap-h, chart)
  if cap != "" { v(4pt); align(center, text(size: 11pt, fill: p.muted)[#cap]) }
})
#let l-hbar(p, n, s) = chart-slide(p, n, s,
  hbar-chart(p, s.at("categories", default: ()), s.at("series", default: ())))
#let l-bar(p, n, s) = chart-slide(p, n, s,
  bar-chart(p, s.at("categories", default: ()), s.at("series", default: ()),
    callouts: s.at("callouts", default: ())))
#let l-line(p, n, s) = chart-slide(p, n, s,
  line-chart(p, s.at("categories", default: ()), s.at("series", default: ())))
#let l-donut(p, n, s) = chart-slide(p, n, s,
  align(center + horizon,
    donut-chart(p, s.at("slices", default: ()),
      style: s.at("style", default: "donut"),
      center_label: s.at("center_label", default: ""))))

// ---- Matrix: product rows × feature columns, ✓/✗/– marks --------------------
#let l-matrix(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(14pt)
  let cols = s.at("columns", default: ())
  let rows = s.at("rows", default: ())
  let hl = s.at("highlight", default: "")
  let body = ()
  for r in rows {
    let name = r.at("name", default: "")
    let win = hl == name
    body.push(
      if win {
        block(fill: grad, radius: 8pt, stroke: none, inset: (x: 10pt, y: 7pt),
          width: 100%, height: 100%,
          align(horizon, text(size: 12.5pt, weight: 600, fill: white)[#name]))
      } else {
        text(size: 12.5pt, weight: 600, fill: p.fg)[#name]
      })
    for c in r.at("cells", default: ()) {
      body.push(mark-glyph(p, c))
    }
  }
  let cnt = calc.max(rows.len(), 1)
  let tbl-avail = CH - 3.8cm
  let hdr = 0.9cm
  table(
    columns: (3.2cm, ..(1fr,) * calc.max(cols.len(), 1)),
    rows: (hdr, ..((tbl-avail - hdr) / cnt,) * cnt),
    stroke: none,
    inset: (x: 10pt, y: 9pt),
    align: (left + horizon, center + horizon),
    fill: (col, row) => if row == 0 { p.zebra } else { none },
    table.header("", ..cols.map(c => text(size: 12.5pt, weight: 600, fill: p.fg2)[#c])),
    ..body,
  )
  let cap = s.at("caption", default: "")
  if cap != "" { v(5pt); text(size: 11pt, fill: p.muted)[#cap] }
})

// ---- Stack: vertical layer diagram, optional annotated side panel -----------
#let l-stack(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(0.6cm)
  let layers = s.at("layers", default: ())
  let hl = s.at("highlight", default: "")
  let side = s.at("side", default: "")
  let cnt = calc.max(layers.len(), 1)
  let gap = 0.35cm
  let lh = (CH - 3.4cm - gap * (cnt - 1)) / cnt
  let stack-body = stack(spacing: gap,
    ..layers.enumerate().map(((i, l)) => {
      let name = l.at("label", default: "")
      let note = l.at("note", default: "")
      let win = hl == name or hl == str(i + 1)
      let win-fg = if win { if p.dark { white } else { anthracite } } else { p.fg }
      let win-note = if win { if p.dark { rgb(255, 255, 255, 80%) } else { rgb("#4A4A4A") } } else { p.muted }
      let inner = align(left + horizon, box(width: 100%,
        grid(columns: (1fr, auto), column-gutter: 12pt, align: horizon,
          text(size: 15pt, weight: 600, fill: win-fg)[#name],
          if note != "" { text(size: 11.5pt, fill: win-note)[#note] } else { [] })))
      if win { box(height: lh, grad-card(p, align(center + horizon, inner), inset: 13pt)) }
      else { box(height: lh, glass(p, align(center + horizon, inner), inset: 13pt)) }
    }))
  if side != "" {
    grid(columns: (1.15fr, 1fr), column-gutter: 1.1cm, rows: (CH - 3.4cm,),
      align(horizon, stack-body),
      align(horizon, glass(p, text(size: 13pt, fill: p.fg2, eval(side, mode: "markup")), inset: 16pt)))
  } else {
    align(center + horizon, box(width: 23cm, stack-body))
  }
})

// ---- Do / Don't: two mark panels --------------------------------------------
#let l-do-dont(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(16pt)
  let left = s.at("left", default: (:))
  let right = s.at("right", default: (:))
  let col(data, mark, markcolor) = glass(p, {
    text(size: 16pt, weight: 600, fill: p.fg)[#data.at("heading", default: "")]
    v(10pt)
    for pt in data.at("points", default: ()) {
      grid(columns: (auto, 1fr), column-gutter: 9pt,
        text(size: 14pt, weight: 600, fill: markcolor)[#mark],
        text(size: 13.5pt, fill: p.fg2)[#pt])
      v(7pt)
    }
  }, inset: 16pt)
  grid(
    columns: (1fr, 1fr), column-gutter: 0.9cm, rows: (7.6cm,),
    col(left, "✓", rgb("#38A76F")),
    col(right, "✕", peach),
  )
})

// ---- Numbered definition cards (01–06) ---------------------------------------
#let l-numbered-cards(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(18pt)
  let cards = s.at("cards", default: ())
  let per = calc.max(s.at("per_row", default: 3), 1)
  let rows = calc.ceil(cards.len() / per)
  grid(
    columns: (1fr,) * per,
    rows: ((CH - 3.6cm) / rows,) * rows,
    column-gutter: 0.7cm, row-gutter: 0.7cm,
    ..cards.enumerate().map(((i, c)) => glass(p, {
      grad-text([#{ if i + 1 < 10 { "0" } }#str(i + 1)], size: 26pt)
      v(8pt)
      text(size: 15.5pt, weight: 600, fill: p.fg)[#c.at("title", default: "")]
      let b = c.at("body", default: "")
      if b != "" { v(6pt); text(size: 12pt, fill: p.fg2)[#b] }
    }, inset: 16pt)))
})

// ---- Persona cards: avatar, pains/needs, quote, message band -----------------
#let l-persona(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(14pt)
  let ppl = s.at("people", default: ())
  grid(
    columns: (1fr,) * calc.max(ppl.len(), 1), column-gutter: 0.7cm, rows: (8.9cm,),
    ..ppl.map(person => {
      let pains = person.at("pains", default: ())
      let needs = person.at("needs", default: ())
      let quote = person.at("quote", default: "")
      let msg = person.at("message", default: "")
      let micro(items) = {
        for b in items {
          grid(columns: (auto, 1fr), column-gutter: 6pt,
            text(size: 10pt, fill: purple)[•],
            text(size: 10.5pt, fill: p.fg2)[#b])
          v(4pt)
        }
      }
      glass(p, box(height: 100%, grid(
        rows: (auto, 1fr, auto, auto), row-gutter: 8pt,
        grid(columns: (auto, 1fr), column-gutter: 10pt,
          avatar(p, person, d: 1.7cm),
          {
            text(size: 14.5pt, weight: 600, fill: p.fg)[#person.at("name", default: "")]
            v(1pt)
            text(size: 11pt, fill: p.muted)[#person.at("role", default: "")]
          }),
        grid(columns: (1fr, 1fr), column-gutter: 10pt,
          {
            text(size: 9pt, weight: 600, tracking: 1pt, fill: p.muted)[
              #upper(person.at("pains_label", default: "Pains"))]
            v(4pt)
            micro(pains)
          },
          {
            text(size: 9pt, weight: 600, tracking: 1pt, fill: p.muted)[
              #upper(person.at("needs_label", default: "Needs"))]
            v(4pt)
            micro(needs)
          }),
        if quote == "" { [] } else {
          block(stroke: (left: 2pt + purple),
            inset: (left: 8pt, top: 2pt, bottom: 2pt),
            text(size: 11pt, style: "italic", fill: p.fg2)[“#quote”])
        },
        if msg == "" { [] } else {
          block(fill: grad, radius: 8pt, stroke: none, inset: (x: 10pt, y: 6pt),
            text(size: 10.5pt, weight: 500, fill: white)[#msg])
        },
      )), inset: 14pt)
    })
  )
})

// ---- Findings: dense numbered list with detail lines -------------------------
#let l-findings(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(0.7cm)
  let items = s.at("items", default: ())
  let cnt = calc.max(items.len(), 1)
  grid(
    rows: ((CH - 3.4cm) / cnt,) * cnt, row-gutter: 0.2cm,
    ..items.enumerate().map(((i, it)) => grid(
      columns: (auto, 1fr), column-gutter: 12pt,
      num-badge(p, str(i + 1), d: 1.05cm),
      {
        text(size: 14.5pt, weight: 600, fill: p.fg)[#it.at("title", default: "")]
        let d = it.at("detail", default: "")
        if d != "" { v(2pt); text(size: 12pt, fill: p.fg2)[#d] }
      },
    ))
  )
})

// ---- Q&A: two-column question cards ------------------------------------------
#let l-qa(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(16pt)
  let items = s.at("items", default: ())
  let per = calc.max(s.at("per_row", default: 2), 1)
  let rows = calc.ceil(items.len() / per)
  grid(
    columns: (1fr,) * per,
    rows: ((CH - 3.6cm) / rows,) * rows,
    column-gutter: 0.7cm, row-gutter: 0.7cm,
    ..items.map(it => glass(p, {
      text(size: 13.5pt, weight: 600, fill: p.fg)[
        #text(fill: purple, weight: 600)[Q] #h(6pt) #it.at("q", default: "")]
      v(8pt)
      text(size: 12pt, fill: p.fg2, eval(it.at("a", default: ""), mode: "markup"))
    }, inset: 14pt)))
})

// ---- Objection → response rows ------------------------------------------------
#let l-objection(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(14pt)
  let rows = s.at("rows", default: ())
  let cnt = calc.max(rows.len(), 1)
  let body = ()
  for r in rows {
    body.push(block(fill: peach.transparentize(88%), radius: 8pt, stroke: none,
      inset: (x: 12pt, y: 8pt), width: 100%, height: 100%,
      align(horizon, text(size: 13pt, weight: 600, fill: p.fg)[
        #r.at("objection", default: "")])))
    body.push(block(inset: (x: 12pt, y: 8pt), width: 100%, height: 100%,
      align(horizon, text(size: 12.5pt, fill: p.fg2,
        eval(r.at("response", default: ""), mode: "markup")))))
  }
  table(
    columns: (6.5cm, 1fr),
    rows: ((CH - 3.4cm) / cnt,) * cnt,
    stroke: none,
    ..body,
  )
})

// ---- Actions: numbered action / owner / status table --------------------------
#let l-actions(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(14pt)
  let rows = s.at("rows", default: ())
  let cnt = calc.max(rows.len(), 1)
  let body = ()
  for (i, r) in rows.enumerate() {
    body.push(num-badge(p, str(i + 1), d: 0.9cm))
    body.push(text(size: 13pt, weight: 500, fill: p.fg)[#r.at("action", default: "")])
    body.push(text(size: 12pt, fill: p.fg2)[#r.at("owner", default: "")])
    body.push(if r.at("status", default: "") == "" { [] } else { badge(p, r.at("status")) })
  }
  table(
    columns: (1.2cm, 1fr, 4.5cm, auto),
    rows: ((CH - 3.6cm) / cnt,) * cnt,
    stroke: none,
    inset: (x: 8pt, y: 6pt),
    align: (horizon, left + horizon, left + horizon, horizon),
    ..body,
  )
})

// ---- Two (or three) independent text columns -----------------------------------
#let l-twocol(p, n, s) = chrome(p, n, s, {
  title-block(p, s)
  v(0.8cm)
  let cols = s.at("cols", default: ())
  grid(
    columns: (1fr,) * calc.max(cols.len(), 1), column-gutter: 1.2cm, rows: (CH - 3.2cm,),
    ..cols.map(c => {
      let h = c.at("heading", default: "")
      let body = c.at("body", default: "")
      let bullets = c.at("bullets", default: ())
      {
        if h != "" { text(size: 15pt, weight: 600, fill: purple)[#h]; v(8pt) }
        if body != "" { text(size: 13.5pt, fill: p.fg2, eval(body, mode: "markup")) }
        if bullets.len() > 0 { v(6pt); bullet-list(p, bullets, size: 13.5pt) }
      }
    })
  )
})

// ============================================================================
// COMPOSE — free stacking of branded content blocks
//
// `layout: "compose"` takes a `blocks` array; each block is one of the brand
// atoms below, rendered at a compact scale so several fit on one slide. The
// blocks stack top-to-bottom in the content area (above the takeaway band);
// like every layout there is no auto-pagination, so keep the stack short
// enough to fit.
// ============================================================================
#let compose-block(p, b) = {
  let t = b.at("type", default: "text")
  if t == "text" {
    text(size: 13pt, fill: p.fg2, eval(b.at("content", default: ""), mode: "markup"))
  } else if t == "bullets" {
    bullet-list(p, b.at("items", default: ()), size: 13pt)
  } else if t == "callout" {
    glass-fit(p, {
      let l = b.at("label", default: "")
      if l != "" {
        text(size: 10pt, weight: 600, tracking: 1.2pt, fill: purple)[#upper(l)]
        v(5pt)
      }
      text(size: 12.5pt, fill: p.fg, eval(b.at("text", default: ""), mode: "markup"))
    }, inset: 14pt)
  } else if t == "cards" {
    let cs = b.at("cards", default: ())
    grid(
      columns: (1fr,) * calc.max(cs.len(), 1), column-gutter: 0.6cm, rows: (4.4cm,),
      ..cs.map(c => feature-card(p, c, style: b.at("card_style", default: "glass"), h: 4.4cm)))
  } else if t == "table" {
    data-table(p, b.at("columns", default: ()), b.at("rows", default: ()), dense: true)
  } else if t == "hbar" {
    hbar-chart(p, b.at("categories", default: ()), b.at("series", default: ()), h: 4.4cm)
  } else if t == "bar" {
    bar-chart(p, b.at("categories", default: ()), b.at("series", default: ()), h: 4.4cm)
  } else if t == "line" {
    line-chart(p, b.at("categories", default: ()), b.at("series", default: ()), h: 4.4cm)
  } else if t == "donut" {
    align(center + horizon,
      donut-chart(p, b.at("slices", default: ()), style: b.at("style", default: "donut")))
  } else if t == "image" {
    let src = b.at("src", default: "")
    if src != "" {
      fitted-image(src, 3.6cm)
      let cap = b.at("caption", default: "")
      if cap != "" { v(3pt); align(center, text(size: 10.5pt, fill: p.muted)[#cap]) }
    }
  } else if t == "quote" {
    grad-text([“#b.at("text", default: "")”], size: 17pt)
    let a = b.at("attribution", default: "")
    if a != "" { v(4pt); align(center, text(size: 11pt, fill: p.fg2)[— #a]) }
  } else if t == "stats" {
    let st = b.at("stats", default: ())
    grid(
      columns: (1fr,) * calc.max(st.len(), 1), column-gutter: 0.6cm,
      ..st.map(x => align(center, {
        grad-text(x.at("value", default: ""), size: 26pt, weight: 600)
        v(3pt)
        text(size: 11.5pt, weight: 600, fill: p.fg)[#x.at("label", default: "")]
      })))
  } else if t == "process" {
    process-row(p, b.at("steps", default: ()))
  } else if t == "divider" {
    rect(width: 100%, height: 0.8pt, fill: p.card-line, stroke: none)
  } else {
    text(size: 11pt, fill: p.muted)[unknown block type: #t]
  }
}

#let l-compose(p, n, s) = chrome(p, n, s, {
  let blocks = s.at("blocks", default: ())
  if s.at("title", default: "") != "" {
    title-block(p, s)
    v(12pt)
  }
  for (i, b) in blocks.enumerate() {
    compose-block(p, b)
    if i < blocks.len() - 1 { v(0.5cm) }
  }
})

// ============================================================================
// LAYOUT REGISTRY
//
// Every layout normalised to the (p, n, s) signature so the render loop in
// template.typ can dispatch with a single lookup. `content` is the default
// for unknown layout names.
// ============================================================================
#let all-layouts = (
  cover: (p, n, s) => l-cover(p, s),
  section: (p, n, s) => l-section(p, n, s),
  agenda: (p, n, s) => l-agenda(p, n, s),
  content: (p, n, s) => l-content(p, n, s),
  cards: (p, n, s) => l-cards(p, n, s),
  split: (p, n, s) => l-split(p, n, s),
  stats: (p, n, s) => l-stats(p, n, s),
  dashboard: (p, n, s) => l-dashboard(p, n, s),
  comparison: (p, n, s) => l-comparison(p, n, s),
  process: (p, n, s) => l-process(p, n, s),
  quote: (p, n, s) => l-quote(p, n, s),
  media: (p, n, s) => l-media(p, s, n),
  image: (p, n, s) => l-image(p, n, s),
  team: (p, n, s) => l-team(p, n, s),
  table: (p, n, s) => l-table(p, n, s),
  diagram: (p, n, s) => l-diagram(p, n, s),
  statement: (p, n, s) => l-statement(p, n, s),
  bignumber: (p, n, s) => l-bignumber(p, n, s),
  logos: (p, n, s) => l-logos(p, n, s),
  gallery: (p, n, s) => l-gallery(p, n, s),
  timeline: (p, n, s) => l-timeline(p, n, s),
  cta-panel: (p, n, s) => l-cta-panel(p, n, s),
  closing: (p, n, s) => l-closing(p, n, s),
  hbar: (p, n, s) => l-hbar(p, n, s),
  bar: (p, n, s) => l-bar(p, n, s),
  line: (p, n, s) => l-line(p, n, s),
  donut: (p, n, s) => l-donut(p, n, s),
  matrix: (p, n, s) => l-matrix(p, n, s),
  stack: (p, n, s) => l-stack(p, n, s),
  do-dont: (p, n, s) => l-do-dont(p, n, s),
  numbered-cards: (p, n, s) => l-numbered-cards(p, n, s),
  persona: (p, n, s) => l-persona(p, n, s),
  findings: (p, n, s) => l-findings(p, n, s),
  qa: (p, n, s) => l-qa(p, n, s),
  objection: (p, n, s) => l-objection(p, n, s),
  actions: (p, n, s) => l-actions(p, n, s),
  twocol: (p, n, s) => l-twocol(p, n, s),
  compose: (p, n, s) => l-compose(p, n, s),
)
