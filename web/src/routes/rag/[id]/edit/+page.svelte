<script lang="ts">
	import { page } from '$app/state';
	import CollectionEditorPage from '$lib/components/rag/CollectionEditorPage.svelte';
</script>

<CollectionEditorPage id={page.params.id} />
