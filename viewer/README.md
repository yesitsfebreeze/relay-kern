# kern graph viewer

A hot-reloadable Vue + Vite app that renders the kern knowledge graph live. It
talks to the daemon's HTTP data API (`/graph`, default `http://127.0.0.1:7700`),
so iterating on the UI never rebuilds kern.

## Dev (hot reload)

```bash
cd viewer
npm install
npm run dev        # http://localhost:5173, HMR
```

Vite proxies `/graph` to the kern daemon, so there's no CORS and no kern
rebuild. Point at another daemon with `KERN_URL=http://host:7700 npm run dev`.

## Build (static)

```bash
npm run build      # -> dist/  (serve with any static host, or `npm run preview`)
```

## What it shows

Force-directed graph: nodes = thoughts (colored by kern, sized by heat; hover
for kind · heat · confidence · text), links = reason edges. Node positions
persist across refreshes — only new thoughts settle in. The HUD lets you change
the refresh interval or refresh on demand.

The kern daemon exposes the data only (`GET /graph` → `{nodes, links, kerns}`);
this app is the UI.
