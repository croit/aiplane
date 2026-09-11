export function safeReturnTo(target: string | null): string | null {
	if (!target || !target.startsWith('/') || target.startsWith('//')) return null;
	if (target === '/login' || target.startsWith('/login?')) return null;
	return target;
}

export function loginPageUrl(returnTo: string): string {
	return `/login?${new URLSearchParams({ return_to: returnTo })}`;
}
