# STATUS: llm-generated, unreviewed — pending native-speaker QA

agent-output-withheld = Je ne peux pas donner cette réponse, car elle mentionnait des informations que je n’ai pas pu vérifier pour vous. Veuillez reformuler votre question ou contacter l’assistance.
agent-output-redacted = [supprimé]
agent-error-name-taken = Un agent personnel portant ce nom existe déjà dans votre compte. Choisissez un autre nom.
agent-error-name-invalid = Saisissez un nom de 80 caractères maximum, sans caractère de contrôle.
agent-error-active-limit = La limite d’agents personnels actifs est atteinte. Désactivez un agent avant d'en créer un autre.
agent-error-not-found = Cet agent n'est pas accessible pour vous.
agent-error-id-missing = L'identifiant de l’agent manque dans la requête.
agent-memory-error-kind = Choisissez préférence, projet ou fait comme type de souvenir.
agent-memory-error-empty = Le contenu du souvenir ne peut pas être vide.
agent-memory-error-id-missing = L'identifiant de l’agent ou du souvenir manque dans la requête.

# What a website visitor reads next to the secure field when an identity
# verifier (#95) asks for the code it sent. Worded the same whether or not
# the address is registered, so it reveals neither.
agent-verifier-code-sent = Si cette adresse est enregistrée chez nous, nous y avons envoyé un code. Saisissez le code ici.
agent-verifier-code-again = Veuillez saisir le code que nous vous avons envoyé.

# What a website visitor reads when the public agent endpoint refuses a
# request (#92): the agent's budget is spent, or a rate limit was hit.
agent-embed-unavailable = Cet assistant est temporairement indisponible. Veuillez réessayer plus tard ou utiliser les autres moyens de contact du site.
agent-embed-rate-limited = Vous envoyez des messages plus vite que cet assistant ne les accepte. Veuillez patienter { $seconds } secondes, puis réessayer.

agent-embed-decision-for-staff = Cette demande attend la décision d’un membre de notre équipe. Vous verrez la réponse ici dès qu’il aura décidé.
agent-embed-not-waiting = L’assistant n’attend pas de réponse de votre part pour le moment. Veuillez recharger la conversation.
agent-embed-message-waiting = Votre message précédent attend encore l’assistant. Veuillez attendre sa réponse avant d’en envoyer un autre.

agent-a2a-input-required = Un service partenaire a besoin d'une information supplémentaire pour continuer. Saisissez-la ici ; elle n'est transmise qu'à ce service.
agent-error-inactive = Cet agent est désactivé. Activez-le avant de démarrer une conversation.
agent-action-chat = Discuter
agent-action-chat-opening = Ouverture de la discussion…
agent-activity-session-started = Discussion avec l’agent démarrée.

agent-workspace-error-unavailable = Le stockage persistant de l’agent est indisponible. Demandez à votre administrateur de vérifier le service sandbox.
agent-workspace-error-conflict = L’espace de travail est occupé ou a changé. Rechargez et réessayez lorsque l’agent est inactif.
agent-workspace-error-not-found = Cette entrée est introuvable. Rechargez le répertoire.
agent-workspace-error-too-large = Le fichier ou le répertoire dépasse la limite de l’espace de travail. Utilisez un fichier ou un répertoire plus petit.
agent-workspace-error-invalid = La demande est invalide. Vérifiez le chemin et réessayez.
agent-workspace-error-failed = L’opération a échoué. Rechargez et réessayez.

agent-workspace-heading = Espace de travail
agent-workspace-intro = Les fichiers de cet espace de travail sont conservés entre les exécutions de l’agent. Téléchargez-les pour les consulter.
agent-workspace-reload = Recharger l’espace de travail
agent-workspace-unavailable = Le stockage persistant est indisponible. Demandez à votre administration de vérifier le service sandbox.
agent-workspace-busy = Cet agent utilise son espace de travail. Les fichiers peuvent changer. L’envoi de fichiers, la création de dossiers et la suppression sont disponibles lorsqu’il est inactif.
agent-workspace-empty = Ce dossier est vide.
agent-workspace-root = Racine de l’espace de travail
agent-workspace-folder = Dossier
agent-workspace-size-missing = Taille indisponible
agent-workspace-modified = Modifié { $when }
agent-workspace-upload = Envoyer un fichier
agent-workspace-file = Fichier
agent-workspace-upload-limit = Choisissez un fichier de 50 Mio maximum.
agent-workspace-create-folder = Créer un dossier
agent-workspace-folder-name = Nom du dossier
agent-workspace-delete = Supprimer
agent-workspace-delete-confirm = Supprimer « { $name } » ? Les dossiers doivent être vides.
agent-workspace-more = Charger davantage de fichiers
agent-workspace-error-load = L’espace de travail n’a pas pu être chargé. Rechargez-le et réessayez.
agent-workspace-error-save = La modification n’a pas pu être enregistrée. Rechargez l’espace de travail et réessayez.
agent-workspace-notice-saved = Espace de travail mis à jour.
