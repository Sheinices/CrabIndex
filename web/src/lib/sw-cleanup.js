/** Removes service workers and caches left by older releases (the app no longer works offline). */
export async function removeLegacyServiceWorkers(nav = globalThis.navigator, cacheStorage = globalThis.caches) {
  try {
    if (nav?.serviceWorker?.getRegistrations) {
      const regs = await nav.serviceWorker.getRegistrations()
      await Promise.all(regs.map((reg) => reg.unregister()))
    }
    if (cacheStorage?.keys) {
      const keys = await cacheStorage.keys()
      await Promise.all(keys.map((key) => cacheStorage.delete(key)))
    }
  } catch {
    /* best effort */
  }
}
