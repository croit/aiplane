// croit LLM Gateway — SPA service worker (root scope).
//
// Ports the legacy root-scope worker's Web Push half (crates/session-core/
// assets/sw.js) to the SPA: turn-complete notifications + notification
// clicks. The SPA's assets are content-hashed with immutable server cache
// headers, so the worker carries no fetch cache — installability and push
// are its whole job; every request passes straight through to the network.
//
// The gateway's push payload is {title, body, url, tag}, with url already
// the conversation path the SPA serves (/chat/{id}). There is no longer a
// second UI to disambiguate from — the SPA IS the root — so the URL is used
// as sent. It used to be rewritten to an `/app` prefix, which no route has
// answered since the SPA moved to the root: clicking a notification landed
// on the 404 page, and because the suppression check compared the open tab's
// `/chat/{id}` against `/app/chat/{id}` it never matched, so every finished
// turn notified even while the user was looking straight at it.

self.addEventListener('install', () => self.skipWaiting());
self.addEventListener('activate', () => self.clients.claim());

/// The route to focus for a pushed conversation URL. Same-origin absolute
/// paths only — a payload is server-controlled, but this is what a click
/// navigates to, so anything else falls back to the app root.
function spaUrl(url) {
  if (typeof url !== 'string' || !url.startsWith('/') || url.startsWith('//')) return '/';
  return url;
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
  const target = (event.notification.data && event.notification.data.url) || '/';
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
