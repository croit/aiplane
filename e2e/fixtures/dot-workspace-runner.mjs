import http from "node:http";

const stores = new Map();
let available = true;
const port = Number(process.env.DOT_WORKSPACE_FIXTURE_PORT ?? 9099);
const maxBytes = 50 * 1024 * 1024;

function reply(response, status, body, contentType = "application/json") {
    response.writeHead(status, { "content-type": contentType });
    response.end(body === undefined ? undefined : contentType === "application/json" ? JSON.stringify(body) : body);
}

function validPath(path, root = false) {
    return typeof path === "string" && (root && path === "" || path.length > 0
        && !path.includes("\0") && !path.startsWith("/")
        && path.split("/").every((part) => part && part !== "." && part !== ".."));
}

async function jsonBody(request) {
    const chunks = [];
    let size = 0;
    for await (const chunk of request) {
        size += chunk.length;
        if (size > 96 * 1024 * 1024) throw Object.assign(new Error("bounded body"), { status: 413 });
        chunks.push(chunk);
    }
    return JSON.parse(Buffer.concat(chunks).toString("utf8"));
}

const server = http.createServer(async (request, response) => {
    try {
        const url = new URL(request.url, "http://127.0.0.1");
        if (request.method === "GET" && url.pathname === "/healthz") {
            reply(response, 200, { status: "ok", workspace: available, egress: false });
            return;
        }
        if (request.method === "POST" && url.pathname === "/__control") {
            const control = await jsonBody(request);
            if (typeof control.available === "boolean") available = control.available;
            if (control.dot_id && typeof control.busy === "boolean") {
                const store = stores.get(control.dot_id);
                if (store) store.busy = control.busy;
            }
            reply(response, 200, { available });
            return;
        }
        if (request.method !== "POST" || url.pathname !== "/workspace") {
            reply(response, 503, { error: "This fixture does not execute sandbox jobs." });
            return;
        }
        if (!available) { reply(response, 503, { error: "storage unavailable" }); return; }
        const { workspace, operation } = await jsonBody(request);
        if (!workspace?.owner_user_id || !/^[0-9a-f-]{36}$/.test(workspace.dot_id ?? "") || !operation?.kind) {
            reply(response, 400, { error: "invalid fixture workspace request" }); return;
        }
        let store = stores.get(workspace.dot_id);
        if (store && store.owner !== workspace.owner_user_id) { reply(response, 409, { error: "owner mismatch" }); return; }
        if (operation.kind === "retire") {
            if (store?.busy) { reply(response, 409, { error: "workspace busy" }); return; }
            stores.delete(workspace.dot_id);
            reply(response, 204); return;
        }
        if (!store) {
            store = { owner: workspace.owner_user_id, busy: false, entries: new Map() };
            stores.set(workspace.dot_id, store);
        }
        const path = operation.path;
        if (!validPath(path, operation.kind === "list")) { reply(response, 400, { error: "invalid path" }); return; }
        if (!["list", "read"].includes(operation.kind) && store.busy) { reply(response, 409, { error: "workspace busy" }); return; }
        const parent = path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "";
        if (parent && store.entries.get(parent)?.kind !== "directory") { reply(response, 404, { error: "missing folder" }); return; }
        const entry = store.entries.get(path);
        if (operation.kind === "list") {
            if (path && entry?.kind !== "directory") { reply(response, 404, { error: "missing folder" }); return; }
            const prefix = path ? `${path}/` : "";
            const entries = [...store.entries].filter(([key]) => key.startsWith(prefix) && !key.slice(prefix.length).includes("/"))
                .map(([key, value]) => ({ name: key.slice(prefix.length), kind: value.kind, size_bytes: value.bytes?.length ?? null, modified_at: value.modified }))
                .sort((a, b) => a.name < b.name ? -1 : a.name > b.name ? 1 : 0)
                .filter((item) => !operation.after || item.name > operation.after);
            const page = entries.slice(0, 100);
            reply(response, 200, { entries: page, next_cursor: entries.length > 100 ? page.at(-1).name : null, busy: store.busy }); return;
        }
        if (operation.kind === "read") {
            if (entry?.kind !== "file") { reply(response, 404, { error: "missing file" }); return; }
            reply(response, 200, entry.bytes, "application/octet-stream"); return;
        }
        if (operation.kind === "write") {
            if (entry?.kind === "directory") { reply(response, 409, { error: "directory exists" }); return; }
            const encoded = operation.content_b64;
            if (typeof encoded !== "string" || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(encoded)) {
                reply(response, 400, { error: "invalid base64" }); return;
            }
            const bytes = Buffer.from(encoded, "base64");
            if (bytes.length > maxBytes) { reply(response, 413, { error: "file too large" }); return; }
            store.entries.set(path, { kind: "file", bytes, modified: new Date().toISOString() });
        } else if (operation.kind === "create_folder") {
            if (entry) { reply(response, 409, { error: "entry exists" }); return; }
            store.entries.set(path, { kind: "directory", modified: new Date().toISOString() });
        } else if (operation.kind === "remove") {
            if (!entry) { reply(response, 404, { error: "missing entry" }); return; }
            if ([...store.entries.keys()].some((key) => key.startsWith(`${path}/`))) {
                reply(response, 409, { error: "folder is not empty" }); return;
            }
            store.entries.delete(path);
        } else { reply(response, 400, { error: "unsupported operation" }); return; }
        reply(response, 204);
    } catch (error) {
        reply(response, error.status ?? 400, { error: "invalid fixture request" });
    }
});

server.listen(port, "127.0.0.1", () => {
    console.log(`Dot workspace fixture: http://127.0.0.1:${server.address().port}`);
});
for (const signal of ["SIGTERM", "SIGINT"]) process.on(signal, () => server.close());
