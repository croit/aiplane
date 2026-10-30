# Data classification

Data classification ranks conversations and model pools by sensitivity. This page describes the data model, how the gateway resolves levels and how an existing installation is migrated. Administrators configure it through [system settings](admin/settings.md#data-classification).

## Data model

All tables belong to one migration (`0090_data_classification.sql`).

| Table | Content |
|---|---|
| `classification_levels` | `id`, unique `rank` (0 is the lowest), `name`, `description`, `color`, and the marking fields `marking_text`, `mip_label_id`, `filename_tag`. A fresh installation holds one level, `public`. |
| `classification_data_classes` | A class a System One model can recognise: `level_id` (the level the content needs), `action` (`raise`, `confirm`, `block`), and the rule (`rule_question`, positive and negative examples, `rule_threshold`). A class with an empty `rule_question` has no detector. |
| `classification_terms`, `classification_markings` | Reserved columns of the scheme that no code reads yet. |
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
