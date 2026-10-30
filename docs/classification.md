# Data classification

Data classification ranks conversations and model pools by sensitivity. This page describes the data model, how the gateway resolves levels and how an existing installation is migrated. Administrators configure it through [system settings](admin/settings.md#data-classification).

## Data model

All tables belong to one migration (`0090_data_classification.sql`).

| Table | Content |
|---|---|
| `classification_levels` | `id`, unique `rank` (0 is the lowest), `name`, `description`, `color`, and the marking fields `marking_text` (visible text), `mip_label_id` (Purview label GUID) and `filename_tag`. A fresh installation holds one level, `public`. |
| `classification_data_classes` | A class a System One model can recognise: `level_id` (the level the content needs), `action` (`raise`, `confirm`, `block`), and the rule (`rule_question`, positive and negative examples, `rule_threshold`). A class with an empty `rule_question` has no detector. |
| `classification_terms` | Reserved: the word-list terms of a data class. No code reads it yet. |
| `classification_markings` | `(kind, value) -> level_id` with `kind` one of `mip` (a Purview label GUID or name), `tscp`, `sensitivity` and `stamp` (visible stamp text). The marking library below reads it; no request path does yet. The filename tag is not a row: it is the level's `filename_tag`. |
| `classification_assignments` | `(kind, id) -> level_id`, `over` (`hide` or `gate`). `kind` is `pool`, `tool`, `mcp`, `connector` or `collection`; only `pool` is read. A pool's `id` is its name; triggers follow a rename and remove the row on delete. |
| `chat_session_settings.level_id` | The conversation's level. `NULL` means the lowest level. |
| `chat_classification_events` | One row per raise or lowering: `from_level`, `to_level`, `cause` (`default`, `migration`, `user`, `system_one:<class id>`), `actor`, `reason`. |

## Resolution

- The scheme is loaded into an immutable `Scheme` snapshot held in `AppState` and replaced after boot and after every settings save. A change to the tables takes effect on the next settings save or restart.
- With one level, `Scheme::is_active` is false: no level is shown, no model is hidden and the detector asks nothing.
- A model's level is the lowest rank of the pools serving it. Pools without an assignment are at rank 0.
- A new conversation stores `classification.default_level` only when its rank is above the lowest, with the event cause `default`.
- `raise` runs in an immediate transaction and does nothing unless the target is higher. `lower` needs `classification.lowering = with_reason` and a non-empty reason. Both write an event.

## Migration of an existing installation

An installation that used the earlier GDPR and NDA pool flags, the content guard or confidential conversations is converted by the migration:

- Levels `internal`, `confidential` and `strictly_confidential` are created above `public`.
- A pool declared GDPR and NDA covered is assigned `strictly_confidential`, a GDPR-only pool `internal`; every other pool, including one declared NDA-only, is `public`.
- The content guard's two questions become the data classes `personal_data` (level `internal`) and `nda` (level `confidential`), kept only while the guard was enabled. A guard action of allow becomes a class action of `raise` without a rule, so nothing is asked; confirm becomes `confirm`; deny becomes `block`.
- Confidential conversations are moved to `confidential` with an event of cause `migration`.
- `chat.confidential.default` becomes `classification.default_level = confidential`; the guard's model and mode become `classification.system_one_model` and `classification.mode`.
- The old settings, the pool flag columns and `chat_session_settings.confidential` are removed.

An installation that never used any of these keeps the single `public` level and sees no change.

## Markings, patterns and word lists

The library in `aiplane-features` (`server::classification`) reads what a file or text carries. It takes bytes and strings and returns values; it reads no database and nothing in a request path calls it yet.

### Digital markings

`read_markings(filename, mime, bytes)` returns the markings found and whether the file is `unreadable`. The format is recognised by content (zip package, `%PDF-`) as well as by extension.

| Format | Read |
|---|---|
| OOXML (docx, xlsx, pptx, macro and template variants) | `docProps/custom.xml`: a label for each `MSIP_Label_<GUID>_Enabled` that is `true` (case-insensitive), with its `_Name`; the TSCP property `urn:bails:IntellectualProperty:BusinessAuthorizationCategory:Name`. `docMetadata/LabelInfo.xml`: each `clbl:label` with `enabled="1"` and not `removed="1"`, by GUID only. |
| OpenDocument (odt, ods, odp, templates) | `meta.xml`: the `meta:user-defined` TSCP category name. |
| PDF | The last `<x:xmpmeta>` packet in the file, `MSIP_Label_<GUID>_Enabled/_Name` elements in any namespace prefix. |
| E-mail (`.eml`, `message/rfc822`) | The `Sensitivity` header and the `msip_labels` header (`MSIP_Label_<GUID>_Enabled=True; …_Name=…`). |

A GUID is lower case with no braces. A visible stamp is not read from bytes: it is looked for in the file's text (below).

A file is `unreadable` when:
- it is an OLE/CFB container where a Word, Excel, PowerPoint or OpenDocument file is expected (an encrypted Office file; the legacy `.doc`, `.xls` and `.ppt` formats are OLE too, and their labels are not read),
- it has an Office extension but is not a readable zip,
- a label part inflates past 8 MiB or is not well-formed XML,
- a PDF names `/Encrypt`.

Limits:
- Every zip part is read through a cap of 8 MiB and a package may hold at most 10,000 entries, so a zip bomb costs a bounded read.
- A PDF whose XMP is in a compressed stream reads as having no label. Only an uncompressed packet is read.

### Resolution

`resolve(markings, filename, mapping)` maps what was found to one level:
- a Purview label by GUID first, by name second, against the `mip` rows (case-insensitive, braces ignored),
- a TSCP category and a `Sensitivity` value by value against the `tscp` and `sensitivity` rows,
- the file name by the levels' `filename_tag`: a stem ending in `-<tag>` (case-insensitive) is that level; of overlapping tags the highest rank wins,
- a marking with no row is the *unknown-label level* (`classification.unknown_label_level`), an unreadable file the *unreadable level*,
- of several, the highest rank.

The result lists content-free causes for the audit: `marking:mip:<guid>`, `marking:tscp`, `marking:sensitivity`, `marking:filename`, `marking:unmapped`, `marking:unreadable`.

### Text and visible stamps

`detection_text(filename, mime, bytes, cap)` gives the text a text detector reads, cut at `cap` bytes:
- plain text, Markdown, CSV, JSON and other text: decoded as UTF-8; HTML: tags stripped,
- Word, Excel and PowerPoint packages and OpenDocument files: the character data of their XML parts, Word headers before the body so a header stamp falls in the stamp window,
- PDF: the text layer (`pdf::extract_text`); a scan has no text,
- e-mail: subject and body.

Images and unknown binaries have no text. `stamp_matches(text, rules)` returns the levels whose `stamp` row appears, case-insensitively and on word boundaries, in the first 4 KiB of the text.

### Patterns

`find_patterns(text, ids)` returns the ids of the built-in patterns found, never the matched text. Class patterns name these ids:

| Id | Matches |
|---|---|
| `email` | An e-mail address. |
| `iban` | 15 to 34 letters and digits, optionally in groups separated by single spaces, with a valid ISO 13616 mod-97 check. |
| `payment_card` | 13 to 19 digits, optionally in groups separated by single spaces or dashes, with a valid Luhn check. |
| `de_tax_id` | A German Steuer-ID: 11 digits (groups allowed), no leading zero, exactly one digit repeated two or three times among the first ten and never three in a row, and a valid ISO 7064 MOD 11,10 check digit. |

### Word lists

`TermMatcher` matches many terms at once, case-insensitively and on Unicode word boundaries (a letter, digit or underscore is part of a word). It is built once per list and reused. A term shorter than 3 characters is ignored.

`parse_term_upload(bytes, csv_column)` reads an upload for the preview: plain text is one term per line; with a column name the file is CSV with a header row, the delimiter `,` (or `;` when the header has a semicolon and no comma) and quoted fields. It returns the terms to match, the duplicates (case-insensitive, first spelling kept) and the terms under 3 characters, which are left out of the list.

## Output marking

`stamp_output(filename, mime, bytes, level, scheme, options)` marks a generated file with a level. A level of rank 0, or one with no marking text, no filename tag and no label id, leaves the file as it is. The options (`classification.output.visible`, `.metadata`, `.filename`) switch the visible marking, the metadata and the file name tag.

| Format | Metadata | Visible |
|---|---|---|
| docx, xlsx, pptx and variants | `docProps/custom.xml` gets `MSIP_Label_<GUID>_Enabled/SetDate/Method/Name/SiteId/ActionId/ContentBits` for the level's `mip_label_id` (the name is the level's name, `SiteId` is `classification.mip_tenant_id`); other `MSIP_Label_*` properties are removed; the content type and the package relationship are added when missing; `docMetadata/LabelInfo.xml` is rewritten when the file has one. Only for a level with a `mip_label_id`. | Word only: a bold centred paragraph with the marking text in the header of every section. |
| odt, ods, odp and templates | The TSCP user-defined properties `BusinessAuthorizationCategory:Name` (the level's name) and `:Identifier` (its id) in `meta.xml`. | None. |
| PDF | An incremental update: a new XMP stream with the label properties and a new revision of the catalog pointing `/Metadata` at it. Only for a level with a `mip_label_id`. | None. |
| Markdown, text | None. | The marking text as the first line, then a blank line. |
| HTML | `<meta name="classification">` with the level's name before `</head>`. | A banner element with the marking text at the start of the body. |
| CSV, JSON, code, images and anything else | None. | None. |

- Every other part of a zip is copied as its compressed bytes, in its original order, so it is byte for byte what it was.
- The Word header: a header part already referenced by a section gets the paragraph first; the first section gets a header of its own for each type (default, first page, even pages) it lacks, and later sections inherit as in Word. A document without a section gets one.
- A PDF is left unchanged, and gets the file name tag only, when it is encrypted, ends in a cross-reference stream or a hybrid table, has a catalog this cannot edit, or has an existing XMP stream that is compressed. An existing uncompressed XMP stream keeps its other properties.
- The file name tag is `{stem}-{tag}.{ext}`. A tag of any level already on the stem is replaced, never stacked. A restamp replaces the first-line text, the HTML banner and meta tag, the Word header paragraph and the label properties the same way.
- A document that cannot be rewritten (a damaged package, a part this cannot edit) is an error and the caller decides whether to deliver it unmarked.
- MIP protection (encryption) is out of scope: the label is metadata only.
