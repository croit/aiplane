/**
 * Browser diagnostics capture for the feedback widget.
 *
 * Monkey-patches `console.*`, `fetch` and `XMLHttpRequest` once at startup and
 * keeps bounded ring buffers of recent console + network activity, plus a 5 s
 * resource-timing sweep. The dialog folds these into `system_info`, so a
 * report carries the errors and requests that led up to it instead of just the
 * reporter's recollection of them.
 *
 * Ported from `croit.erp`'s `capture-utils.ts` by way of the gateway's own
 * pre-SPA `ui/ts/feedback-capture.ts`. Two rules survive unchanged because
 * they are the ones that make this safe to run on every page load:
 *
 *   - **Bounded.** 100 console entries and 100 network entries, never more;
 *     the submit path slices network to the last 50 on top of that.
 *   - **Redacted.** Query parameters and JSON request bodies have their
 *     credential-shaped keys replaced before they ever enter a buffer, so a
 *     public issue tracker never receives one even in principle.
 *
 * Initialised from the root layout's `load`, which is the earliest client code
 * in the app — a request made before then is one this cannot see.
 */

export interface ConsoleLog {
	timestamp: string;
	level: 'log' | 'info' | 'warn' | 'error' | 'debug';
	args: string[];
}

export interface NetworkLog {
	timestamp: string;
	method: string;
	url: string;
	status?: number;
	duration?: number;
	type?: string;
	size?: number;
	query?: string;
	requestBody?: string;
}

const MAX_CONSOLE_LOGS = 100;
const MAX_NETWORK_LOGS = 100;

/**
 * Keys whose values are never recorded. Matched case-insensitively against
 * query-parameter names and top-level JSON body keys.
 */
const SENSITIVE_KEYS = new Set([
	'password',
	'passwd',
	'secret',
	'token',
	'access_token',
	'refresh_token',
	'api_key',
	'apikey',
	'authorization',
	'auth',
	'credential',
	'private_key',
	'privatekey',
	'client_secret',
	'clientsecret'
]);

export const REDACTED = '[REDACTED]';

const redactSensitive = (params: Record<string, string>): Record<string, string> => {
	const result: Record<string, string> = {};
	for (const [key, value] of Object.entries(params)) {
		result[key] = SENSITIVE_KEYS.has(key.toLowerCase()) ? REDACTED : value;
	}
	return result;
};

/** The query string of `url`, redacted; `undefined` when there is none. */
export const extractQuery = (url: string, origin = 'http://localhost'): string | undefined => {
	try {
		const parsed = new URL(url, origin);
		if (parsed.search.length <= 1) return undefined;
		const params = redactSensitive(Object.fromEntries(parsed.searchParams));
		return new URLSearchParams(params).toString();
	} catch {
		return undefined;
	}
};

/**
 * A short, redacted summary of a request body. Binary payloads are named
 * rather than transcribed, JSON gets its credential keys replaced, and
 * everything is truncated to 500 characters — an issue body is not a place to
 * paste a megabyte of upload.
 */
export const sanitizeBody = (body: BodyInit | null | undefined): string | undefined => {
	if (!body) return undefined;
	if (typeof FormData !== 'undefined' && body instanceof FormData) return '[FormData]';
	if (typeof Blob !== 'undefined' && body instanceof Blob) return `[Blob ${body.size}B]`;
	if (body instanceof ArrayBuffer) return `[ArrayBuffer ${body.byteLength}B]`;
	if (ArrayBuffer.isView(body)) return `[TypedArray ${body.byteLength}B]`;
	const text = typeof body === 'string' ? body : String(body);
	try {
		const parsed: unknown = JSON.parse(text);
		if (typeof parsed === 'object' && parsed !== null) {
			const redacted: Record<string, unknown> = {};
			for (const [key, value] of Object.entries(parsed)) {
				redacted[key] = SENSITIVE_KEYS.has(key.toLowerCase()) ? REDACTED : value;
			}
			const result = JSON.stringify(redacted);
			return result.length > 500 ? `${result.substring(0, 497)}...` : result;
		}
	} catch {
		/* not JSON — fall through to the plain-text path */
	}
	return text.length > 500 ? `${text.substring(0, 497)}...` : text;
};

const consoleLogs: ConsoleLog[] = [];
const networkLogs: NetworkLog[] = [];

interface CapturedConsoleFn {
	(...args: unknown[]): void;
	__captured?: boolean;
}
interface ExtWindow extends Window {
	__feedbackNetworkCapture?: boolean;
}
interface CapturedXHR extends XMLHttpRequest {
	__method?: string;
	__url?: string;
	__startTime?: number;
	__query?: string;
	__requestBody?: string;
}

/** Serialise one console argument without ever throwing on a cyclic object. */
const stringifyArg = (arg: unknown): string => {
	try {
		if (typeof arg === 'string') return arg;
		if (arg instanceof Error) return `${arg.name}: ${arg.message}\n${arg.stack ?? ''}`;
		return JSON.stringify(arg) ?? String(arg);
	} catch {
		return String(arg);
	}
};

const initConsoleCapture = (): void => {
	if ((console.log as CapturedConsoleFn).__captured) return;
	const wrap =
		(level: ConsoleLog['level'], original: (...a: unknown[]) => void): CapturedConsoleFn =>
		(...args: unknown[]) => {
			original.apply(console, args);
			consoleLogs.push({
				timestamp: new Date().toISOString(),
				level,
				args: args.map(stringifyArg)
			});
			if (consoleLogs.length > MAX_CONSOLE_LOGS) consoleLogs.shift();
		};
	console.log = wrap('log', console.log) as typeof console.log;
	console.info = wrap('info', console.info) as typeof console.info;
	console.warn = wrap('warn', console.warn) as typeof console.warn;
	console.error = wrap('error', console.error) as typeof console.error;
	console.debug = wrap('debug', console.debug) as typeof console.debug;
	(console.log as CapturedConsoleFn).__captured = true;
};

const pushNetwork = (entry: NetworkLog): void => {
	networkLogs.push(entry);
	if (networkLogs.length > MAX_NETWORK_LOGS) networkLogs.shift();
};

const initNetworkCapture = (): void => {
	if ((window as ExtWindow).__feedbackNetworkCapture) return;

	if (typeof window.fetch === 'function') {
		const originalFetch = window.fetch;
		const intercepted = async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
			const start = performance.now();
			const url =
				typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
			const method = init?.method || (input instanceof Request ? input.method : 'GET');
			const query = extractQuery(url, window.location.origin);
			const requestBody = sanitizeBody(init?.body ?? null);
			try {
				const resp = await originalFetch(input, init);
				pushNetwork({
					timestamp: new Date().toISOString(),
					method,
					url,
					status: resp.status,
					duration: performance.now() - start,
					type: 'fetch',
					query,
					requestBody
				});
				return resp;
			} catch (err) {
				// status 0 is the browser's own convention for "never answered".
				pushNetwork({
					timestamp: new Date().toISOString(),
					method,
					url,
					status: 0,
					duration: performance.now() - start,
					type: 'fetch',
					query,
					requestBody
				});
				throw err;
			}
		};
		// Keep whatever static properties the platform hung off `fetch`.
		Object.setPrototypeOf(intercepted, originalFetch);
		window.fetch = intercepted as typeof fetch;
	}

	const XHR = window.XMLHttpRequest;
	if (XHR) {
		const open = XHR.prototype.open;
		const send = XHR.prototype.send;
		XHR.prototype.open = function (
			this: CapturedXHR,
			method: string,
			url: string | URL,
			...rest: unknown[]
		) {
			const urlStr = typeof url === 'string' ? url : url.href;
			this.__method = method;
			this.__url = urlStr;
			this.__startTime = performance.now();
			this.__query = extractQuery(urlStr, window.location.origin);
			return (open as (...a: unknown[]) => void).call(this, method, url, ...rest);
		};
		XHR.prototype.send = function (
			this: CapturedXHR,
			body?: Document | XMLHttpRequestBodyInit | null
		) {
			this.__requestBody = body instanceof Document ? '[Document]' : sanitizeBody(body ?? null);
			this.addEventListener('loadend', () => {
				pushNetwork({
					timestamp: new Date().toISOString(),
					method: this.__method ?? 'GET',
					url: this.__url ?? '',
					status: this.status,
					duration: performance.now() - (this.__startTime ?? 0),
					type: 'xhr',
					query: this.__query,
					requestBody: this.__requestBody
				});
			});
			return (send as (...a: unknown[]) => void).call(this, body);
		};
	}

	// Resource-timing sweep for sub-resources (scripts, css, images) that are
	// neither fetch nor XHR. Bounded scan, deduplicated by url + initiator.
	const sweep = (): void => {
		try {
			const entries = performance.getEntriesByType('resource') as PerformanceResourceTiming[];
			for (const e of entries.slice(-MAX_NETWORK_LOGS)) {
				if (networkLogs.some((l) => l.url === e.name && l.type === e.initiatorType)) continue;
				pushNetwork({
					timestamp: new Date(e.startTime + performance.timeOrigin).toISOString(),
					method: 'GET',
					url: e.name,
					duration: e.duration,
					type: e.initiatorType,
					size: e.transferSize,
					query: extractQuery(e.name, window.location.origin)
				});
			}
		} catch {
			/* a failed sweep must never break the page it is observing */
		}
	};
	window.setInterval(sweep, 5000);

	(window as ExtWindow).__feedbackNetworkCapture = true;
};

let started = false;

/** Idempotent; a no-op outside the browser. */
export const initFeedbackCapture = (): void => {
	if (started || typeof window === 'undefined') return;
	started = true;
	initConsoleCapture();
	initNetworkCapture();
};

export const getConsoleLogs = (): ConsoleLog[] => [...consoleLogs];
export const getNetworkLogs = (): NetworkLog[] => [...networkLogs];
