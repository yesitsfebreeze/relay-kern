<script setup>
import { ref, onMounted, onBeforeUnmount } from 'vue'
import ForceGraph3D from '3d-force-graph'
import * as THREE from 'three'

const stats = ref('loading…')
const err = ref('')
const focusLabel = ref('')
const graphEl = ref(null)

let G = null
let timer = null
let raw = { nodes: [], links: [], kerns: [] }
let kernsById = {}
const objById = new Map() // id -> node object (persist positions)
let topoKey = ''
let focusId = null

function colorFor(id) {
  let h = 0
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0
  return `hsl(${h % 360}, 65%, 60%)`
}

// Descendant kern ids of `id` (inclusive) — for focus highlighting.
function subtree(id) {
  const out = new Set([id])
  const walk = (k) => {
    const m = kernsById[k]
    if (!m) return
    for (const c of m.children || []) { out.add(c); walk(c) }
  }
  walk(id)
  return out
}

// Build the whole-network graph: entity nodes + kern "sphere" hubs, linked by
// reason edges (entity↔entity), membership (entity→its kern), and hierarchy
// (kern→parent kern) — so the recursive sphere structure lays out in 3D.
function build() {
  const nodes = []
  const ids = new Set()
  const reuse = (n) => {
    const ex = objById.get(n.id)
    if (ex) { Object.assign(ex, n); return ex }
    objById.set(n.id, n); return n
  }
  for (const e of raw.nodes) { const n = reuse({ ...e, _t: 'entity' }); nodes.push(n); ids.add(n.id) }
  for (const k of raw.kerns) { const n = reuse({ id: k.id, label: k.label, count: k.count, named: k.named, parent: k.parent, _t: 'kern' }); nodes.push(n); ids.add(n.id) }
  for (const id of [...objById.keys()]) if (!ids.has(id)) objById.delete(id)

  const links = []
  for (const l of raw.links) {
    const s = l.source.id ?? l.source, t = l.target.id ?? l.target
    if (ids.has(s) && ids.has(t)) links.push({ source: s, target: t, _t: 'reason' })
  }
  for (const e of raw.nodes) if (ids.has(e.kern)) links.push({ source: e.id, target: e.kern, _t: 'member' })
  for (const k of raw.kerns) if (k.parent && ids.has(k.parent)) links.push({ source: k.id, target: k.parent, _t: 'hier' })

  return { nodes, links, ids }
}

function applyHighlight() {
  const sub = focusId ? subtree(focusId) : null
  const inFocus = (n) => !sub ? true : (n._t === 'kern' ? sub.has(n.id) : sub.has(n.kern))
  G.nodeColor((n) => {
    const base = n._t === 'kern' ? colorFor(n.id) : (colorFor(n.kern || n.id))
    if (!sub) return base
    return inFocus(n) ? base : 'rgba(70,80,92,0.15)'
  })
  G.linkColor((l) => {
    if (!sub) return l._t === 'member' ? 'rgba(90,110,130,0.12)' : 'rgba(120,140,160,0.22)'
    const s = l.source.id ?? l.source, t = l.target.id ?? l.target
    const sn = objById.get(s), tn = objById.get(t)
    return (sn && tn && inFocus(sn) && inFocus(tn)) ? 'rgba(150,180,200,0.4)' : 'rgba(60,70,80,0.05)'
  })
}

function render() {
  const { nodes, links } = build()
  const key = nodes.map(n => n.id).sort().join() + '|' + links.map(l => (l.source.id ?? l.source) + '>' + (l.target.id ?? l.target)).sort().join()
  if (key !== topoKey) { topoKey = key; G.graphData({ nodes, links }) }
  applyHighlight()
  const ents = nodes.filter(n => n._t === 'entity').length
  stats.value = `${ents} thoughts · ${raw.links.length} reasons · ${raw.kerns.length} spheres`
}

function focus(id) {
  focusId = id
  focusLabel.value = id ? (kernsById[id]?.label || 'sphere') : ''
  applyHighlight()
  if (id) {
    const n = objById.get(id)
    if (n && n.x != null) {
      const d = 120
      const r = Math.hypot(n.x, n.y, n.z) || 1
      G.cameraPosition({ x: n.x * (1 + d / r), y: n.y * (1 + d / r), z: n.z * (1 + d / r) }, n, 800)
    }
  } else {
    G.zoomToFit(600, 40)
  }
}

async function load() {
  try {
    raw = await (await fetch('/graph')).json()
    kernsById = {}
    for (const k of raw.kerns) kernsById[k.id] = k
    render()
    err.value = ''
  } catch (e) { err.value = String(e) }
}

onMounted(() => {
  G = ForceGraph3D()(graphEl.value)
    .backgroundColor('#06080b')
    .showNavInfo(false)
    .nodeLabel((n) => n._t === 'kern'
      ? `◉ sphere: ${n.label} · ${n.count} thoughts`
      : `${n.kind} · heat ${(+n.heat).toFixed(2)} · conf ${(+n.conf).toFixed(2)}\n${n.label}`)
    .nodeVal((n) => n._t === 'kern' ? 6 : 1 + (+n.heat || 0) * 2)
    .nodeThreeObject((n) => {
      if (n._t !== 'kern') return false
      const r = 7 + Math.cbrt((n.count || 1)) * 4
      return new THREE.Mesh(
        new THREE.SphereGeometry(r, 18, 14),
        new THREE.MeshLambertMaterial({ color: colorFor(n.id), transparent: true, opacity: 0.18, depthWrite: false })
      )
    })
    .nodeThreeObjectExtend(true)
    .linkOpacity(0.3)
    .onNodeClick((n) => { if (n._t === 'kern') focus(n.id); else focus(null) })
    .onBackgroundClick(() => focus(null))
  load()
  timer = setInterval(load, 5000)
})

onBeforeUnmount(() => { if (timer) clearInterval(timer) })
</script>

<template>
  <div class="hud">
    <b>kern</b> network · {{ stats }}
    <span v-if="focusLabel" class="focus">▸ in: {{ focusLabel }} <button @click="focus(null)">show all</button></span>
    <span v-if="err" class="err"> — {{ err }}</span>
  </div>
  <div class="hint">click a sphere to step in · click empty space to zoom out</div>
  <div ref="graphEl" class="graph"></div>
</template>

<style>
.graph { width: 100vw; height: 100vh; }
.hud {
  position: fixed; top: 10px; left: 12px; z-index: 10;
  background: #11151aee; color: #cdd3da; padding: 8px 12px; border-radius: 8px;
  border: 1px solid #222a33; font: 13px system-ui, sans-serif;
  display: flex; gap: 10px; align-items: center;
}
.hud b { color: #7fd1ae; }
.focus { color: #e5c07b; }
.err { color: #e06c75; }
.hint { position: fixed; bottom: 10px; left: 12px; z-index: 10; color: #5a6573; font: 12px system-ui, sans-serif; }
.hud button {
  background: #0b0d10; color: #cdd3da; border: 1px solid #2a323b;
  border-radius: 5px; padding: 1px 7px; cursor: pointer; margin-left: 4px;
}
</style>
