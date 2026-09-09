# Strings owned by `gateway/src/rama_server/pages/scheduled.rs` — the
# scheduled-actions management page (builder form + list) and its full-page
# edit sub-page.

scheduled-heading = Scheduled actions
scheduled-list-empty = No scheduled actions yet. Create one above.

scheduled-edit-heading = Edit scheduled action

scheduled-name-label = Name
scheduled-name-placeholder = e.g. Daily news digest
scheduled-model-label = Model
scheduled-model-placeholder = model id (e.g. gpt-4o-mini)
scheduled-prompt-label = Prompt
scheduled-prompt-placeholder = What should the model do each time it runs?

scheduled-builder-heading = Schedule
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
scheduled-hour-aria = Hour
scheduled-minute-aria = Minute
scheduled-cron-label = Cron expression

scheduled-next-runs-prefix = Next runs:{ " " }

scheduled-err-pick-weekday = Pick at least one weekday.

scheduled-badge-active = active
scheduled-badge-paused = paused
scheduled-next-run = Next run: { $when }
scheduled-pause-title = Pause
scheduled-resume-title = Resume
scheduled-edit-title = Edit
scheduled-delete-title = Delete

# SPA-only: the Svelte /scheduled page's builder form and action list.
scheduled-new-heading = New scheduled action
scheduled-weekdays-label = Weekdays
scheduled-day-of-month-label = Day of month
scheduled-update-preview = Update preview
scheduled-err-invalid-schedule = Invalid schedule.
scheduled-badge-last-run-failed = last run failed
scheduled-delete-confirm = Delete this scheduled action?
