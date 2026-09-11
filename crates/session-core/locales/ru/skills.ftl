# STATUS: llm-generated, unreviewed — pending native-speaker QA

skills-heading = Скиллы
skills-intro-part1 = Инструкции, установленные оператором, которые чат-модель подгружает по требованию через инструмент
skills-intro-part2 = предназначенный для этого. Загрузите архив
skills-intro-part3 = ниже — он доступен сразу, без перезапуска.
skills-empty-loaded = Скиллы пока не загружены. Загрузите архив .skill, чтобы добавить один.
skills-empty-not-configured = Скиллы не настроены. Включите их в /admin/settings (skills.dir) и перезапустите шлюз, чтобы они загрузились.

skills-upload-heading = Добавить скилл
skills-upload-button = Загрузить .skill
skills-loaded-heading = Загруженные скиллы
skills-none-yet = Пока нет
skills-source-prefix = Источник:

skills-download-title = Скачать этот скилл как архив .skill
skills-download-button = Скачать
skills-delete-title = Удалить этот скилл
skills-delete-button = Удалить
skills-granted-to-heading = Доступ предоставлен
skills-granted-config-title = Доступ предоставлен для каждого скилла
skills-choose-access-title = Выберите, каким группам разрешено использовать этот скилл
skills-no-grants-warning = ни одна группа не предоставляет доступ — настроить доступ
skills-edit-access-title = Изменить, каким группам разрешено использовать этот скилл
skills-edit-access-button = Изменить доступ
skills-files-heading = Файлы
skills-files-count = { $count } в комплекте
skills-description-heading = Описание

skills-grant-dialog-heading = Кто может использовать этот скилл?
skills-grant-dialog-desc-part1 = Выберите группы, которым разрешено загружать этот скилл:
skills-grant-dialog-desc-part2 = . Доступ получит каждый участник выбранной группы.
skills-grant-dialog-no-roles-part1 = Группы шлюза не определены. Добавьте записи
skills-grant-dialog-no-roles-part2 = прежде чем предоставлять доступ.

skills-cancel-button = Отмена
skills-save-access-button = Сохранить доступ
skills-from-config-badge = все скиллы
skills-error-no-dir-access = Нет доступа к каталогу навыков — проверьте, что он существует и шлюз может читать и записывать в него:

# Управление скиллами в SPA: сообщение об установке, подтверждение удаления и
# подпись при отсутствии дополнительных прав.
skills-installed = Установлено: { $name }.
skills-delete-confirm = Удалить глобальный скилл { $name } вместе с его правами?
