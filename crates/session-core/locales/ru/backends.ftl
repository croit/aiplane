# STATUS: llm-generated, unreviewed — pending native-speaker QA

backends-heading = Восходящие бэкенды

backends-status-down = недоступен
backends-status-up = активен

backends-inflight-label = в обработке { $load }

# Backend CRUD editor (add/edit/delete backends stored in the DB topology).
backends-apply-changes = Применить изменения
backends-field-name = Название
backends-field-base-url = Базовый URL
backends-field-pool = Пул
backends-field-pool-none = (нет)
backends-save-backend = Сохранить бэкенд
backends-add-backend = Добавить бэкенд
backends-delete-backend = Удалить

backends-field-api-key = Ключ API
backends-field-api-key-keep = оставьте пустым, чтобы сохранить текущий ключ

# An alias that is configured but would not route.
# Save-time check on the aliases textarea.
# Save-time check on the aliases textarea.
# Two states that leave a backend green but unusable.
# Maintenance switch.
backends-status-drained = обслуживание

# Строки бэкендов в SPA: переключатель обслуживания, нагрузка за час и удаление.
backends-drain-button = Вывести из ротации
backends-undrain-button = Вернуть в ротацию
backends-requests-per-hour = { $count } запр./ч
backends-delete-confirm = Удалить бэкенд { $name }? После этого примените топологию.
