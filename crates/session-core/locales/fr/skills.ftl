# STATUS: llm-generated, unreviewed — pending native-speaker QA

skills-heading = Skills
skills-intro-part1 = Instructions installées par l'opérateur que le modèle de chat charge à la demande grâce à l'outil
skills-intro-part2 = prévu à cet effet. Téléversez une archive
skills-intro-part3 = ci-dessous — elle est disponible immédiatement, sans redémarrage.
skills-empty-loaded = Aucun skill chargé pour le moment. Téléversez une archive .skill pour en ajouter un.
skills-empty-not-configured = Les skills ne sont pas configurés. Activez-les dans /admin/settings (skills.dir) puis redémarrez pour les charger.

skills-upload-heading = Ajouter un skill
skills-upload-button = Téléverser .skill
skills-loaded-heading = Skills chargés
skills-none-yet = Aucun pour le moment
skills-source-prefix = Source :

skills-download-title = Télécharger ce skill sous forme d'archive .skill
skills-download-button = Télécharger
skills-delete-title = Supprimer ce skill
skills-delete-button = Supprimer
skills-granted-to-heading = Accordé à
skills-granted-config-title = Accordé pour chaque skill
skills-choose-access-title = Choisissez les groupes autorisés à utiliser ce skill
skills-no-grants-warning = aucun groupe ne l'accorde — définir l'accès
skills-edit-access-title = Modifier les groupes autorisés à utiliser ce skill
skills-edit-access-button = Modifier l'accès
skills-files-heading = Fichiers
skills-files-count = { $count } inclus
skills-description-heading = Description

skills-grant-dialog-heading = Qui peut utiliser ce skill ?
skills-grant-dialog-desc-part1 = Choisissez les groupes autorisés à charger ce skill :
skills-grant-dialog-desc-part2 = . Chaque personne d'un groupe sélectionné y a accès.
skills-grant-dialog-no-roles-part1 = Aucun groupe de passerelle n'est défini. Ajoutez des entrées
skills-grant-dialog-no-roles-part2 = avant de pouvoir accorder l'accès.

skills-cancel-button = Annuler
skills-save-access-button = Enregistrer l'accès
skills-from-config-badge = tous les skills
skills-error-no-dir-access = Pas d’accès au répertoire des compétences — vérifiez qu’il existe et que la passerelle peut y lire et écrire :

# Gestion des skills dans la SPA : message d'installation, confirmation de
# suppression et libellé d'absence d'autorisation.
skills-installed = { $name } installé.
skills-delete-confirm = Supprimer le skill global { $name } et ses autorisations ?
