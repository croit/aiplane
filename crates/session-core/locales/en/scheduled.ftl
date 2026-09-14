# Strings owned by `gateway/src/rama_server/pages/scheduled.rs` — the
# scheduled-actions management page (builder form + list) and its full-page
# edit sub-page.


scheduled-heading = Scheduled actions
scheduled-intro = Run a prompt automatically on a schedule. Each run opens as a new chat you can read here — pick a model, write the prompt, and choose when it should run.
scheduled-create-submit = Create scheduled action
scheduled-list-heading = Your scheduled actions
scheduled-list-empty = No scheduled actions yet. Create one with the button above.

scheduled-back = Back
scheduled-edit-heading = Edit scheduled action
scheduled-save-submit = Save changes

scheduled-name-label = Name
scheduled-name-placeholder = e.g. Daily news digest
scheduled-model-label = Model
scheduled-model-placeholder = model id (e.g. gpt-4o-mini)
scheduled-gdpr-warning = This model is not GDPR-compliant. Scheduled runs will send your prompt to it automatically — avoid personal data.
scheduled-nda-warning = This model is not covered by a confidentiality agreement. Don't schedule NDA-protected or proprietary material to it.
scheduled-prompt-label = Prompt
scheduled-prompt-placeholder = What should the model do each time it runs?
scheduled-tools-toggle-label = Allow tools (web search, RAG, attachments) — same as in chat
scheduled-reuse-toggle-label = Reuse the previous run's chat — each run continues the same conversation
scheduled-reuse-rounds-prefix = send last
scheduled-reuse-rounds-aria = Rounds of history to replay
scheduled-reuse-rounds-suffix = rounds

scheduled-builder-heading = Schedule
scheduled-mode-hourly = Hourly
scheduled-mode-daily = Daily
scheduled-mode-weekly = Weekly
scheduled-mode-monthly = Monthly
scheduled-mode-advanced = Advanced
scheduled-weekday-mon = Mon
scheduled-weekday-tue = Tue
scheduled-weekday-wed = Wed
scheduled-weekday-thu = Thu
scheduled-weekday-fri = Fri
scheduled-weekday-sat = Sat
scheduled-weekday-sun = Sun
scheduled-on-day-label = On day
scheduled-of-every-month = of every month
scheduled-at-label = At
scheduled-hour-aria = Hour
scheduled-minute-aria = Minute
scheduled-of-every-hour = of every hour
scheduled-timezone-label = Timezone
scheduled-cron-label = Cron expression
scheduled-cron-help = Five fields: minute hour day-of-month month day-of-week.

scheduled-no-upcoming-runs = No upcoming runs.
scheduled-next-runs-prefix = Next runs:{ " " }

scheduled-err-pick-weekday = Pick at least one weekday.
scheduled-err-enter-cron = Enter a cron expression.


scheduled-toast-not-found = No such scheduled action.

scheduled-badge-active = active
scheduled-badge-paused = paused
scheduled-status-paused = Paused
scheduled-next-run = Next run: { $when }
scheduled-no-upcoming-run = No upcoming run
scheduled-last-success = Last: ✓ { $when }
scheduled-last-failure = Last: ✗ { $when }
scheduled-pause-title = Pause
scheduled-resume-title = Resume
scheduled-edit-title = Edit
scheduled-delete-title = Delete
scheduled-delete-confirm = Delete this scheduled action?
scheduled-preview-summary = { $summary } ({ $timezone })
scheduled-preview-next-runs = Next runs: { $runs }
scheduled-last-error = Last error: { $error }

# The list page's per-schedule links into what it produced, and the run
# history behind them (`/scheduled/{id}/runs`).
scheduled-create-heading = New scheduled action
scheduled-new-page-title = New scheduled action
scheduled-edit-named-heading = Edit { $name }
scheduled-toast-created = Scheduled action created.
scheduled-toast-saved = Scheduled action saved.
scheduled-badge-reuses-chat = one conversation
scheduled-open-chat = Open chat
scheduled-open-chats = { $count ->
    [one] { $count } chat
   *[other] { $count } chats
}
scheduled-open-runs = { $count ->
    [one] { $count } run
   *[other] { $count } runs
}
scheduled-never-run = Has not run yet
scheduled-runs-page-title = Run history
scheduled-runs-heading = Runs · { $name }
scheduled-runs-intro = Every recorded fire, newest first, with the chat it opened.
scheduled-runs-empty = No runs yet. This action has not fired since it was created.
scheduled-run-open = open chat
scheduled-run-status-ok = ok
scheduled-run-status-error = error
scheduled-run-status-pending = running
