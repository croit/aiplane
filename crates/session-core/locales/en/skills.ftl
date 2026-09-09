# Strings owned by `gateway/src/rama_server/pages/skills.rs` — the
# `/admin/skills` viewer + manager (upload, delete, grants).
#
# A few keys (the `-part1`/`-part2`/`-part3` suffixed ones) are sentence
# fragments split around an inline `<code>`/`<span>` element in the
# template; the Rust call site re-joins them with explicit space
# literals, so these values intentionally carry no leading/trailing
# whitespace of their own (Fluent's parser isn't guaranteed to preserve
# it on a single-line value).

skills-heading = Skills
skills-empty-loaded = No skills loaded yet. Upload a .skill archive to add one.

skills-upload-button = Upload .skill

skills-download-title = Download this skill as a .skill archive
skills-download-button = Download
skills-delete-title = Remove this skill
skills-delete-button = Delete
skills-edit-access-button = Edit access

skills-cancel-button = Cancel
skills-save-access-button = Save access

# SPA skill manager: upload toast, delete prompt, and the empty grant label.
skills-installed = Installed { $name }.
skills-delete-confirm = Remove the global skill { $name } and its grants?
skills-no-extra-grants = no extra grants
