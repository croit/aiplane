const PROTOCOL_CLAIMS = new Set(['iss', 'aud', 'exp', 'iat', 'nbf', 'jti', 'nonce', 'at_hash', 'c_hash', 's_hash', 'auth_time', 'azp', 'sid', 'typ', 'acr', 'amr']);
const CLAIM_SEPARATOR = '\u001f';

export interface SetupClaimChoice {
	claim: string;
	value: string;
	label: string;
}

export function setupClaimChoices(claims: Record<string, unknown>): SetupClaimChoice[] {
	const choices: SetupClaimChoice[] = [];
	for (const [claim, raw] of Object.entries(claims)) {
		if (PROTOCOL_CLAIMS.has(claim)) continue;
		const values = Array.isArray(raw) ? raw : [raw];
		for (const value of values) if (typeof value === 'string' && value !== '') choices.push({ claim, value, label: `${claim} = ${value}` });
	}
	return choices.sort((left, right) => left.claim.localeCompare(right.claim) || left.value.localeCompare(right.value));
}

export function encodeSetupClaim(claim: string, value: string): string {
	return `${claim}${CLAIM_SEPARATOR}${value}`;
}

export function decodeSetupClaim(encoded: string): [string, string] | null {
	const separator = encoded.indexOf(CLAIM_SEPARATOR);
	if (separator < 1 || separator === encoded.length - 1) return null;
	return [encoded.slice(0, separator), encoded.slice(separator + 1)];
}
