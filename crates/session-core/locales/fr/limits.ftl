# Éditeur d'administration des limites de débit / quotas (/admin/limits).
limits-heading = Limites de débit et quotas
limits-add-heading = Ajouter ou mettre à jour une limite
limits-field-subject = S'applique à
limits-field-model = Modèle
limits-field-dimension = Limite
limits-field-window = Par
limits-field-value = Valeur
limits-add-submit = Enregistrer la limite
limits-subject-global = Tout le monde (par défaut)
limits-subject-role = Rôle
limits-subject-user = Utilisateur
limits-dim-requests = Requêtes
limits-dim-tokens = Tokens
limits-dim-cost = Coût ({ $cur })
limits-dim-cost-short = Coût
limits-win-hour = Heure
limits-win-day = Jour
limits-win-week = Semaine
limits-win-month = Mois
limits-col-subject = S'applique à
limits-col-scope = Modèle
limits-col-limit = Limite
limits-col-window = Fenêtre
limits-none = Aucune limite configurée — tout le monde est illimité.
limits-all-models = tous les modèles
limits-delete = Supprimer
limits-edit = Modifier
limits-edit-heading = Modifier la limite
limits-edit-submit = Enregistrer les modifications
limits-saved = limite enregistrée pour { $subject }
limits-subject-token = Jeton d'API
limits-subject-system = Agent

# Tableau des règles de la SPA : titre, colonne « géré par » et confirmation.
limits-delete-confirm = Supprimer cette règle ?
limits-intro = Limitez le nombre de requêtes, de tokens ou le montant des dépenses qu'un appelant peut utiliser sur une fenêtre glissante. Les règles sont résolues du plus spécifique au plus général : la règle propre à un utilisateur l'emporte, sinon la plus généreuse de ses rôles, sinon la valeur par défaut globale. Sans règle, tout le monde est illimité. Une règle portant sur un jeton d'API est un plafond supplémentaire vérifié en plus du budget de son propriétaire : elle ne peut que restreindre la dépense de ce jeton. Seuls les pools facturés comptent (les pools auto-hébergés avec enforce_limits = false sont exemptés), et l'ensemble du budget d'un utilisateur est partagé entre ses tokens API, le chat et les exécutions planifiées. Une règle limitée à un modèle (ou à une famille `prefix-*`) remplace l'agrégat dans sa fenêtre : pour un appel de ce modèle, seule la règle correspondante la plus spécifique compte — sauf si l'agrégat est marqué comme plafond global, auquel cas il plafonne en plus.
limits-field-subject-id = Rôle / utilisateur / jeton / agent
limits-field-subject-id-ph = Choisir …
limits-col-value = Valeur
limits-col-actions = Actions
limits-deleted = limite supprimée
limits-field-total = Plafond global
limits-field-total-hint = Si coché, cette règle plafonne en plus — même là où une règle plus spécifique autorise davantage. Sinon, la règle correspondante la plus spécifique l'emporte pour le modèle appelé.
limits-field-model-hint = Vide signifie tous les modèles ; un * final désigne une famille (p. ex. claude-*).
limits-col-kind = Type
limits-kind-total = Global
limits-kind-override = Dérogation
