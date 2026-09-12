<script lang="ts">
	import { page } from '$app/state';
	import AdminConnectorEditorPage from '$lib/components/admin/AdminConnectorEditorPage.svelte';
</script>

<AdminConnectorEditorPage key={page.params.key} />
