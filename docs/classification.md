# Data classification

Data classification ranks conversations and model pools by sensitivity. This page describes the data model, how the gateway resolves levels and how an existing installation is migrated. Administrators configure it at `/admin/classification`; see [Data classification](admin/settings.md#data-classification).

## Data model

All tables belong to one migration (`0090_data_classification.sql`).

| Table | Content |
|---|---|
| `classification_levels` | `id`, unique `rank` (0 is the lowest), `name`, `description`, `color`, and the marking fields `marking_text` (visible text), `mip_label_id` (Purview label GUID) and `filename_tag`. A fresh installation holds one level, `public`. |
| `classification_data_classes` | A class a System One model can recognise: `level_id` (the level the content needs), `action` (`raise`, `confirm`, `block`), and the rule (`rule_question`, positive and negative examples, `rule_threshold`). A class with an empty `rule_question` has no detector. |
| `classification_terms` | The word-list terms of a data class. Read into one `TermMatcher` per class when the scheme loads; a settings save rebuilds them. |
| `classification_markings` | `(kind, value) -> level_id` with `kind` one of `mip` (a Purview label GUID or name), `tscp`, `sensitivity` and `stamp` (visible stamp text). Upload detection reads it. The filename tag is not a row: it is the level's `filename_tag`. |
| `classification_assignments` | `(kind, id) -> level_id`, `over` (`hide` or `gate`). `kind` is `pool`, `tool`, `mcp`, `connector` or `collection`. A pool's `id` is its name; triggers follow a rename and remove the row on delete. A tool's `id` is the tool id, an MCP server's and a connector's the server or connector key, a collection's the numeric collection id (a rename cannot orphan it). A destination without a row is at the lowest level and, for a tool, hidden above it. The rows load with the scheme. |
| `chat_session_settings.level_id` | The conversation's level. `NULL` means the lowest level. |
| `chat_session_shares` | `(session_id, subject_kind, subject_id)`: the people (`user`) and groups (`group`) a conversation is shared with; removed with the conversation. See [Sharing](#sharing). |
| `chat_classification_events` | One row per raise or lowering: `from_level`, `to_level`, `cause` (`default`, `migration`, `user`, `system_one:<class id>`, `pattern:<class id>`, `terms:<class id>`, `marking:*` as in [Resolution](#resolution-1), `retrieval`, `fork`, `scheme_change`), `actor`, `reason`. |

## Resolution

- The scheme is loaded into an immutable `Scheme` snapshot held in `AppState` and replaced after boot and after every settings save. The admin API reloads it after every change; a change made to the tables by other means takes effect on the next settings save or restart.
- With one level, `Scheme::is_active` is false: no level is shown, no model is hidden and the detector asks nothing.
- A model's level is the lowest rank of the pools serving it. Pools without an assignment are at rank 0.
- A new conversation stores `classification.default_level` only when its rank is above the lowest, with the event cause `default`.
- `raise` runs in an immediate transaction and does nothing unless the target is higher. `lower` needs `classification.lowering = with_reason` and a non-empty reason. Both write an event.

## Administration API

All endpoints need administrator rights and live under `/api/v0/admin/classification`. A refused request answers `400` with the code `invalid_classification` and the offending field (`field`, e.g. `levels[2].id` or `policy.link_share_max_level`).

| Endpoint | Purpose |
|---|---|
| `GET /api/v0/admin/classification` | The whole scheme (`active`, `levels`, `classes` with their word-list state, `markings`, `policy`), the `destinations` that can be rated (every pool, tool with its `egress` flag, connector, knowledge collection), the `system_one_models` with `cleared_for_highest` and the `templates`. Template, level and data class names and descriptions come in the language of the request (`Accept-Language`, then the language cookie) and are the scheme's own data once saved; rule questions and positive/negative examples stay English because the System One model is measured on English prompts, and stamps, label names and filename tags stay as real documents carry them. |
| `PUT /api/v0/admin/classification` | Replaces the scheme atomically in one transaction. |
| `POST /api/v0/admin/classification/classes/{id}/terms` | Multipart `file`: replaces the class's word list and reloads the matchers. `?column=` names the CSV column (without it the file is one term per line); `?preview=1` saves nothing. Answers the preview counts (`count`, `duplicates`, `too_short`, `sample`). |
| `DELETE /api/v0/admin/classification/classes/{id}/terms` | Removes the word list. |
| `POST /api/v0/admin/classification/inspect` | Multipart `file`: the markings the document carries. |
| `POST /api/v0/admin/classification/test` | Multipart `file` or `text`: the level, causes and classes the saved scheme gives the sample. Writes nothing. |
| `GET /api/v0/admin/classification/events` | The latest level changes across conversations, newest first. |

`PUT` validates before it writes:

- levels need unique slug ids and contiguous ranks from 0; every level id used by a class, marking, assignment or policy field must exist;
- class ids are unique slugs, patterns are built-in pattern ids, thresholds lie between 0 and 1;
- a `mip` marking value or a level's Purview label must be a GUID (stored lowercase without braces);
- an assignment must name an existing destination; `gate` is accepted only for destinations that send data out of AIplane; non-egress destinations are always `hide`;
- the System One model must be served by a System One pool and be cleared for the highest level when a class has a question or any destination is set to `gate`; it may be empty only when neither is the case.

Removing a level moves its conversations as described under [Data classification](admin/settings.md#data-classification) and writes one `scheme_change` event per conversation. The policy is stored in the `settings.classification.*` rows of `app_settings`, so the runtime reads it through the settings mechanism.

## Sharing

A conversation is read by its owner and by:

- anyone signed in, while it is shared by link and its level is at or below `classification.link_share_max_level` (an empty or unknown value is the lowest level; with one level there is no limit);
- the users listed in `chat_session_shares` (`subject_kind` `user`, the user id);
- the members of the groups listed there (`subject_kind` `group`, the group id, matched against the reader's groups).

`runtime::chat_sharing::can_read` is the one function that decides, and every read goes through it: the conversation, its live event stream, exports, documents and canvas, attachments and the fork source. The link ceiling is evaluated on each read, so raising a conversation above it stops the link immediately without touching the stored flag. A delegated child conversation is private to its owner. Only the owner changes shares.

| Endpoint | Purpose |
|---|---|
| `GET /api/v0/chat/sessions/{id}/shares` | `{link_allowed, link, subjects[{kind, id, name}]}`. `link` is whether the link is open now. |
| `PUT /api/v0/chat/sessions/{id}/shares` | Body `{link, subjects[{kind, id}]}` replaces the link flag and the list in one transaction. `link: true` above the limit is `409` (`link_sharing_above_limit`); an unknown user or group, or the owner, is `400`. |
| `POST /api/v0/chat/sessions/{id}/share` | The single link toggle, with the same `409` rule. |
| `GET /api/v0/chat/sessions/{id}/share-subjects?q=` | The owner's search for people and groups, in the shape of the agents' share search: nothing below two characters, at most eight of each. |

A fork copies the text and the **level** of its source (audited with the cause `fork`) and starts unshared; forking needs read access to the source.

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

The library in `aiplane-features` (`server::classification`) reads what a file or text carries. It takes bytes and strings and returns values and reads no database; [upload detection](#uploads) and the [tool gate](#tools) call it.

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

## At runtime

Everything below applies to a person's conversation. With one level none of it runs: no file is parsed, no model is asked, no file is marked.

Monitor mode (`classification.mode = monitor`) logs what enforce mode would do and takes no action: nothing is raised, asked or refused by a detector, a collection or the gate. The model filter, the tool filter and the levels a conversation already holds apply in both modes.

### Typed text

Before a message goes to a model, what the person wrote since the last answer is checked for the classes above the conversation's level, by the same code as an upload's text: the built-in patterns and word lists of those classes first, then the System One model. The operator's system prompt and earlier messages are not checked again (they were checked when sent, and the level only rises). The check runs whatever the selected model is cleared for, because the level outlives the turn: it decides the tools, sharing and the next model. A match raises the conversation (`pattern:<class id>`, `terms:<class id>`, `system_one:<class id>`) and the class's action applies. On `/v1/chat/completions` and in agent runs nothing is stored: everything the client sent except the model's earlier answers is checked for the classes above the target model's level, and a match refuses the request (`confirm` asks where a person can answer).

In a person's chat, an approved `confirm` also lets the turn go to a model that is not cleared for the conversation's level while `classification.user_override` is `true` (the default; a missing row reads as `true`). With `false` the approval confirms the content only: the model-clearance check runs as for any turn, and a model that is not cleared refuses the turn with the usual message, so the person has to pick a cleared model. Later rounds of the turn are checked as for a turn without an approval.

### Uploads

When a turn starts, before any model reads the attachments (OCR is a model too), the files attached to the latest message are inspected. Each file gives findings; the conversation rises to the highest one, with a content-free cause.

| Source | Cause |
|---|---|
| Digital marking, visible stamp or file name tag mapped by [Resolution](#resolution-1); unreadable or over-size file (64 MiB) | `marking:*` |
| A built-in pattern of a data class, in the first 1 MiB of text | `pattern:<class id>` |
| A word of a data class's list | `terms:<class id>` |
| The System One model, asked about the first 32 KiB of text for the classes above the conversation's level | `system_one:<class id>` |

The markings are weighed first: a class at or below the level a marking sets is neither asked about nor acted on, just as typed text in a conversation already at that level is not checked for it.

A class's action applies exactly as for typed text: `raise` raises, `confirm` asks the person before the content goes to the model, `block` refuses the turn. A marking always raises. In monitor mode only the System One question is asked and logged. A failure of the System One model refuses the turn in enforce mode; what the markings, patterns and word lists found still raises the conversation first, for an upload as for typed text. The person sees that the checking model could not be reached; the cause is logged.

After the raise the model check runs as before: a model whose level is below the conversation's is refused unless the person just approved the content for it. The check is repeated at the start of every later round, so a level that rises during a turn (a retrieval) ends the turn at the next round instead of sending the new content to an uncleared model. An approval covers the level the conversation had when it was given.

### Retrieval

`rag_search`, `rag_grep` and the document tools raise the conversation to the level assigned to the collection (cause `retrieval`) before they return a result, and only when they return content. If the level cannot be stored the call fails and the result is withheld. Agent runs never raise.

### Tools

Only a tool that can send data out of AIplane (`Tool::egress`, the **Egress** column of [tools-inventory.md](tools-inventory.md)) is a destination with a maximum level (see `classification_assignments`). A tool whose arguments and results stay inside the plane (attachments, canvas documents, memory, scheduling, own knowledge collections, QR codes, typst templates, `enable_tools`) is never filtered or gated. MCP tools, ComfyUI workflows and the sandbox tools always count as egress, and so does any tool that does not say otherwise. Re-evaluated at the start of every round:

- `over = hide`: above its level the tool is not offered, and a call to it is refused with a message naming the conversation's level.
- `over = gate`: the tool stays offered. Before each call above its level the arguments are checked for the classes above the tool's level: the patterns and word lists of those classes, then the System One model. No match: the call runs. A match: with `classification.gate_on_match = ask` the call pauses on the ordinary approval prompt, which names the matched classes; with `block` it is refused with a message naming them. If the System One model cannot be reached the call is refused and the message says so. A call approved on the prompt runs without a second check. Nothing is raised: the arguments leave, the conversation does not change.

An unrated egress tool is at the lowest level and hidden above it. A tool such as `edit_image` has no rule of its own; its assignment decides.

### Generated files

Every file a tool, the sandbox or an MCP spill adds to a conversation goes through `upload_stamped` (`aiplane-runtime`), which marks it with the conversation's level as [Output marking](#output-marking) describes. The architecture test `generated_files_are_stored_only_through_the_stamping_helper` keeps it the only path; the user's own composer upload and the ComfyUI scheduler (images and video, below the runtime layer) are the listed exceptions. A file that cannot be rewritten is stored with the file name tag only and a warning is logged. With one level or at the lowest level the bytes are stored unchanged.

### Side calls

Calls that send conversation content to a model beside the turn check the conversation's level first:

| Side call | Behaviour |
|---|---|
| Chat title | Runs after the conversation's turns have settled, so the level an upload or a retrieval set is known; skipped when the title model is not cleared for it. |
| Compaction summary | Not run when the model is not cleared for the conversation's level. |
| System One detection and gate | Asked only about text that is being checked; the System One model must itself be cleared for the highest level. |
| Voice feedback fields, skill drafting, agent judges and routers | Not conversation content of a person's chat (a dictated note, an operator's description, an agent run, which never ratchets). |

### Messages

Messages the turn raises on its own (refusals, the gate's messages to the model) are rendered in English: the person's interface language is a browser cookie and request header, not stored with the user, so a background turn does not know it.

### Stateless requests

`/v1/chat/completions` and agent runs have no conversation to raise. The System One check runs for the classes above the target model's level: a match with action `raise` or `block` refuses the request, `confirm` asks where a person can answer and refuses otherwise.
