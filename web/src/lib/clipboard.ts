/**
 * Copy `text` once `saved` resolves to `true`.
 *
 * The clipboard write starts inside the click — the browser allows it only
 * with the click's user activation, which an awaited round trip can
 * outlive — but its content waits for the server: a refused save leaves the
 * clipboard as it was rather than holding a link nobody can open.
 */
export async function copyOnceSaved(text: string, saved: Promise<boolean>): Promise<boolean> {
	try {
		if (typeof ClipboardItem === 'undefined') {
			if (!(await saved)) return false;
			await navigator.clipboard.writeText(text);
		} else {
			const blob = saved.then((ok) => (ok ? new Blob([text], { type: 'text/plain' }) : Promise.reject(new Error('nothing was saved'))));
			await navigator.clipboard.write([new ClipboardItem({ 'text/plain': blob })]);
		}
		return true;
	} catch {
		return false;
	}
}
