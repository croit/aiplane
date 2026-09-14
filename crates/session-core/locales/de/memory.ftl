# STATUS: llm-generated, unreviewed — pending native-speaker QA

memory-heading = Speicher
memory-description = Was der Assistent sich über dich merkt, gruppiert nach Art. Füge hier Einträge hinzu, bearbeite oder lösche sie — es ist der Speicher deines Kontos und liegt vollständig in deiner Kontrolle. Die Funktion selbst schaltest du auf der Seite „Tools“ ein oder aus.

memory-add-heading = Erinnerung hinzufügen
memory-kind-aria = Art der Erinnerung
memory-content-placeholder = z. B. Bevorzugt Antworten in metrischen Einheiten

memory-empty = Hier ist noch nichts.
memory-save-button = Speichern
memory-delete-title = Erinnerung löschen

# SPA-only: the Svelte /memory page's inline add/edit form.
memory-kind-preference = Präferenzen
memory-kind-project = Projektkontext
memory-kind-fact = Fakten
memory-content-label = Inhalt
memory-add-button = Merken
memory-delete-confirm = Diese Erinnerung löschen?

# The per-category Add button in each card header; it opens the add dialog
# with that card's kind already selected.
memory-add-short = Hinzufügen

# One line under each card heading saying how that kind reaches the
# assistant: preferences ride in the system context of every conversation,
# while project notes and facts wait to be looked up with `recall`. The
# difference changes which bucket a user files something in, and nothing
# else on the page reveals it.
memory-kind-preference-hint = Immer im Kontext — sie werden ab der ersten Nachricht jeder Unterhaltung mitgeschickt, sodass der Assistent sie unaufgefordert befolgt.
memory-kind-project-hint = Wird bei Bedarf abgerufen — der Assistent schlägt diese Einträge nach, wenn es in der Unterhaltung um deine Arbeit geht.
memory-kind-fact-hint = Wird bei Bedarf abgerufen — der Assistent schlägt diese Einträge nach, sobald sie relevant werden.
