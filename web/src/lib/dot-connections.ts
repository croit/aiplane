import type { DotConnection, DotConnectionChange, DotConnectionMode } from './dots.ts';

export function connectionModes(row: DotConnection): DotConnectionMode[] {
	return [...new Set([...row.supported_modes, row.selected.mode])];
}

export function connectionChange(row: DotConnection, mode: DotConnectionMode): DotConnectionChange | null {
	if (!row.can_select || !row.connector || !row.supported_modes.includes(mode)) return null;
	const credential = mode === 'user' ? row.user_connection : row.principal_connection;
	if (!credential?.connected || credential.errored || credential.needs_reauth || credential.authorizing || !credential.id || !credential.generation) return null;
	if (mode === row.selected.mode && credential.id === row.selected.connection_id && credential.generation === row.selected.connection_generation) return null;
	return { mode, expected_binding_generation: row.selected.binding_generation, connection_id: credential.id, connection_generation: credential.generation };
}

export function selectedConnectionAvailable(row: DotConnection): boolean {
	const credential = row.selected.mode === 'user' ? row.user_connection : row.principal_connection;
	return row.connector !== null && row.supported_modes.includes(row.selected.mode)
		&& credential?.connected === true && !credential.errored && !credential.needs_reauth && !credential.authorizing
		&& credential.id !== null && credential.generation !== null
		&& credential.id === row.selected.connection_id && credential.generation === row.selected.connection_generation;
}
