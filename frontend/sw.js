self.addEventListener('fetch', (event) => {
  // Fuck offline page feature. Pass everything directly straight to the network line.
  event.respondWith(fetch(event.request));
});
