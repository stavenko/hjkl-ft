// Cloudflare Pages advanced-mode worker (deployed as dist/_worker.js).
//
// Its ONLY job is to serve a per-user dynamic web-app manifest so an installed PWA carries the
// user's non-secret account id into its launch URL (`start_url=/?u=<user_id>`) — which is how
// the installed app, in its OWN storage context (separate from the browser on iOS), knows
// which account to log into. Everything else falls through to the static assets unchanged.
//
// `?u` empty → the plain app manifest (start_url "/"), so a normal install still works.

function manifest(u) {
  const m = {
    name: "Renorma",
    short_name: "Renorma",
    start_url: u ? `/?u=${encodeURIComponent(u)}` : "/",
    // Distinct `id` per user so a per-account install is its own app; empty → the base app.
    id: u ? `/app-${u}` : "/",
    display: "standalone",
    background_color: "#ffffff",
    theme_color: "#ffffff",
    orientation: "portrait",
    icons: [
      { src: "/icon-192.png", sizes: "192x192", type: "image/png" },
      { src: "/icon-512.png", sizes: "512x512", type: "image/png" },
    ],
  };
  return m;
}

export default {
  async fetch(request, env) {
    const url = new URL(request.url);
    if (url.pathname === "/manifest.json") {
      const u = url.searchParams.get("u") || "";
      return new Response(JSON.stringify(manifest(u)), {
        headers: {
          "Content-Type": "application/manifest+json",
          "Cache-Control": "no-store",
        },
      });
    }

    const res = await env.ASSETS.fetch(request);

    // Inject `?u=<user_id>` into the served HTML's <link rel="manifest"> so the BROWSER reads
    // the per-user manifest (start_url=/?u=<u>) at page-load — before WASM runs. Without this
    // the link carries no ?u when «Add to Home Screen» snapshots the manifest, so the installed
    // PWA loses the account id (its start_url falls back to "/"). The page URL carries the id
    // (/onboard?u=… on install, /?u=… on launch), and we forward it into the manifest link.
    const u = url.searchParams.get("u") || "";
    const ct = res.headers.get("Content-Type") || "";
    if (u && ct.includes("text/html")) {
      const rewritten = new HTMLRewriter()
        .on('link[rel="manifest"]', {
          element(el) {
            el.setAttribute("href", `/manifest.json?u=${encodeURIComponent(u)}`);
          },
        })
        .transform(res);
      // User-specific → never let a shared cache hand this variant to another account.
      const headers = new Headers(rewritten.headers);
      headers.set("Cache-Control", "no-store");
      return new Response(rewritten.body, {
        status: rewritten.status,
        statusText: rewritten.statusText,
        headers,
      });
    }
    // Everything else → the static site (SPA index fallback handled by Pages/_redirects).
    return res;
  },
};
