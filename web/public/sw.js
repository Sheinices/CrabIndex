// Kill-switch service worker.
// Older CrabIndex releases registered an offline service worker at "/".
// Browsers keep checking this URL for updates: this version wipes every cache,
// unregisters itself and reloads open tabs so they are served by the network again.
self.addEventListener('install', () => {
  self.skipWaiting()
})

self.addEventListener('activate', (event) => {
  event.waitUntil(
    (async () => {
      try {
        const keys = await caches.keys()
        await Promise.all(keys.map((key) => caches.delete(key)))
      } catch {
        /* ignore */
      }
      await self.registration.unregister()
      const clients = await self.clients.matchAll({ type: 'window' })
      for (const client of clients) {
        try {
          client.navigate(client.url)
        } catch {
          /* ignore */
        }
      }
    })(),
  )
})
