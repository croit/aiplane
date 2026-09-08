// croit LLM Gateway — SPA service worker (scope /app/).
//
// Ports the legacy root-scope worker's Web Push half (crates/session-core/
// assets/sw.js) to the SPA: turn-complete notifications + notification
// clicks. The SPA's assets are content-hashed with immutable server cache
// headers, so the worker carries no fetch cache — installability and push
// are its whole job; every request passes straight through to the network.
//
// The gateway's push payload is the legacy-shaped {title, body, url, tag}
// with url pointing at the LEGACY conversation path (/chat/{id}) — the
// server serves both UIs and cannot know which one this browser runs. This
// worker maps it onto the SPA route before focusing a window.

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', () => self.clients.claim());

/// The SPA route for a (possibly legacy-path) conversation URL.
function spaUrl(url) {
  if (typeof url !== 'string' || !url.startsWith('/')) return '/app';
  if (url.startsWith('/chat/')) return `/app${url}`;
  if (url.startsWith('/app')) return url;
  return '/app';
}

function sameConversation(clientUrl, target) {
  try {
    const a = new URL(clientUrl);
    return a.pathname === target;
  } catch (_) {
    return false;
  }
}

self.addEventListener('push', (event) => {
  let data = {};
  try {
    data = event.data ? event.data.json() : {};
  } catch (_) {
    data = {};
  }
  const title = data.title || 'LLM Gateway';
  const target = spaUrl(data.url);
  const options = {
    body: data.body || '',
    // `tag` makes repeated pings for one conversation replace, not stack.
    tag: data.tag || 'gateway-turn',
    icon: '/icons/icon-192.png',
    badge: '/icons/icon-192.png',
    data: { url: target }
  };
  event.waitUntil(
    (async () => {
      const wins = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
      // The server always sends; "am I looking at it?" is decided here.
      // Suppressing when the user is actively viewing this exact
      // conversation is the whole point of the notification.
      const lookingAtIt = wins.some(
        (c) => c.focused && c.visibilityState === 'visible' && sameConversation(c.url, target)
      );
      if (lookingAtIt) return;
      await self.registration.showNotification(title, options);
    })()
  );
});

self.addEventListener('notificationclick', (event) => {
  event.notification.close();
  const target = (event.notification.data && event.notification.data.url) || '/app';
  event.waitUntil(
    (async () => {
      const wins = await self.clients.matchAll({ type: 'window', includeUncontrolled: true });
      for (const c of wins) {
        if (sameConversation(c.url, target)) {
          await c.focus();
          return;
        }
      }
      for (const c of wins) {
        await c.focus();
        await c.navigate(target).catch(() => {});
        return;
      }
      await self.clients.openWindow(target);
    })()
  );
});
