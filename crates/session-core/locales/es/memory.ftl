# STATUS: llm-generated, unreviewed — pending native-speaker QA

memory-heading = Memoria
memory-description = Lo que el asistente recuerda sobre ti, agrupado por tipo. Añade, edita o elimina entradas aquí — es la memoria de tu cuenta y está totalmente bajo tu control. Activa o desactiva la función en sí en la página Herramientas.

memory-add-heading = Añadir un recuerdo
memory-kind-aria = Tipo de recuerdo
memory-content-placeholder = p. ej. Prefiere respuestas en unidades métricas

memory-empty = Aún no hay nada aquí.
memory-save-button = Guardar
memory-delete-title = Eliminar recuerdo

# SPA-only: the Svelte /memory page's inline add/edit form.
memory-kind-preference = Preferencias
memory-kind-project = Contexto del proyecto
memory-kind-fact = Hechos
memory-content-label = Contenido
memory-add-button = Recordar
memory-delete-confirm = ¿Eliminar este recuerdo?

# The per-category Add button in each card header; it opens the add dialog
# with that card's kind already selected.
memory-add-short = Añadir

# One line under each card heading saying how that kind reaches the
# assistant: preferences ride in the system context of every conversation,
# while project notes and facts wait to be looked up with `recall`. The
# difference changes which bucket a user files something in, and nothing
# else on the page reveals it.
memory-kind-preference-hint = Siempre en el contexto — se envían desde el primer mensaje de cada conversación, así que el asistente las aplica sin que se lo pidas.
memory-kind-project-hint = Se consulta cuando hace falta — el asistente busca estas entradas cuando la conversación toca tu trabajo.
memory-kind-fact-hint = Se consulta cuando hace falta — el asistente busca estas entradas en cuanto resultan relevantes.
