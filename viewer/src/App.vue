<script setup>
import { ref, onMounted, onBeforeUnmount } from 'vue'
import ForceGraph3D from '3d-force-graph'
import * as THREE from 'three'
import SpriteText from 'three-spritetext'

const stats = ref('loading…')
const err = ref('')
const focusLabel = ref('')
const graphEl = ref(null)

let G = null
let timer = null
let raw = { nodes: [], links: [], kerns: [] }
let kernsById = {}
const objById = new Map()        // id -> live node object (persists positions)
let builtNodes = []              // current node objects (entities + hubs)
let fields = new Map()           // kernId -> { mesh, label }
let topoKey = ''
let focusId = null

function colorFor(id) {
  let h = 0
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0
  return new THREE.Color(`hsl(${h % 360}, 65%, 60%)`)
}

function subtree(id) {
  const out = new Set([id])
  const walk = (k) => { for (const c of (kernsById[k]?.children || [])) { out.add(c); walk(c) } }
  walk(id)
  return out
}

function build() {
  const nodes = []
  const ids = new Set()
  const reuse = (n) => {
    const ex = objById.get(n.id)
    if (ex) { Object.assign(ex, n); return ex }
    objById.set(n.id, n); return n
  }
  for (const e of raw.nodes) { nodes.push(reuse({ ...e, _t: 'entity' })); ids.add(e.id) }
  // kern hubs: invisible-ish anchors at each field's centre, clickable to enter.
  for (const k of raw.kerns) { nodes.push(reuse({ id: k.id, label: k.label, count: k.count, named: k.named, parent: k.parent, _t: 'kern' })); ids.add(k.id) }
  for (const id of [...objById.keys()]) if (!ids.has(id)) objById.delete(id)

  const links = []
  for (const l of raw.links) {
    const s = l.source.id ?? l.source, t = l.target.id ?? l.target
    if (ids.has(s) && ids.has(t)) links.push({ source: s, target: t, _t: 'reason' })
  }
  // scaffolding forces (hidden): pull entities into their kern, kerns to parent.
  for (const e of raw.nodes) if (ids.has(e.kern)) links.push({ source: e.id, target: e.kern, _t: 'member' })
  for (const k of raw.kerns) if (k.parent && ids.has(k.parent)) links.push({ source: k.id, target: k.parent, _t: 'hier' })

  builtNodes = nodes
  return { nodes, links, ids }
}

// One translucent field sphere + floating label per kern, added to the scene.
function rebuildFields() {
  for (const { mesh, label } of fields.values()) { G.scene().remove(mesh); G.scene().remove(label) }
  fields.clear()
  for (const k of raw.kerns) {
    const mesh = new THREE.Mesh(
      new THREE.SphereGeometry(1, 20, 16),
      new THREE.MeshLambertMaterial({ color: colorFor(k.id), transparent: true, opacity: 0.10, depthWrite: false })
    )
    const label = new SpriteText(k.named ? k.label : '◦')
    label.color = colorFor(k.id).getStyle()
    label.textHeight = 5
    label.material.depthWrite = false
    G.scene().add(mesh)
    G.scene().add(label)
    fields.set(k.id, { mesh, label })
  }
}

// Each tick: shape every field sphere to enclose its member thoughts and float
// its label above. This makes fields overlap exactly where their thoughts mix.
function updateFields() {
  const byKern = new Map()
  for (const n of builtNodes) {
    if (n._t === 'entity' && n.x != null) {
      if (!byKern.has(n.kern)) byKern.set(n.kern, [])
      byKern.get(n.kern).push(n)
    }
  }
  const sub = focusId ? subtree(focusId) : null
  for (const [id, { mesh, label }] of fields) {
    const pts = (byKern.get(id) || []).slice()
    const hub = objById.get(id)
    if (hub && hub.x != null) pts.push(hub)
    if (!pts.length) { mesh.visible = false; label.visible = false; continue }
    let cx = 0, cy = 0, cz = 0
    for (const p of pts) { cx += p.x; cy += p.y; cz += p.z }
    cx /= pts.length; cy /= pts.length; cz /= pts.length
    let r = 8
    for (const p of pts) r = Math.max(r, Math.hypot(p.x - cx, p.y - cy, p.z - cz))
    r += 7
    mesh.visible = true; label.visible = true
    mesh.position.set(cx, cy, cz); mesh.scale.set(r, r, r)
    label.position.set(cx, cy + r + 5, cz)
    const dim = sub && !sub.has(id)
    mesh.material.opacity = dim ? 0.02 : (focusId === id ? 0.18 : 0.10)
    label.material.opacity = dim ? 0.15 : 1
  }
}

function applyHighlight() {
  const sub = focusId ? subtree(focusId) : null
  G.nodeColor((n) => {
    const c = (n._t === 'kern' ? colorFor(n.id) : colorFor(n.kern || n.id)).getStyle()
    if (!sub) return c
    const inF = n._t === 'kern' ? sub.has(n.id) : sub.has(n.kern)
    return inF ? c : 'rgba(70,80,92,0.12)'
  })
  G.linkColor((l) => {
    if (l._t !== 'reason') return 'rgba(0,0,0,0)'
    if (!sub) return 'rgba(150,170,190,0.35)'
    const s = objById.get(l.source.id ?? l.source), t = objById.get(l.target.id ?? l.target)
    const inF = (n) => n && sub.has(n.kern)
    return inF(s) && inF(t) ? 'rgba(170,200,220,0.55)' : 'rgba(60,70,80,0.05)'
  })
}

function render() {
  const { nodes, links } = build()
  const key = nodes.map(n => n.id).sort().join() + '|' + links.map(l => (l.source.id ?? l.source) + '>' + (l.target.id ?? l.target)).sort().join()
  if (key !== topoKey) { topoKey = key; G.graphData({ nodes, links }); rebuildFields() }
  applyHighlight()
  stats.value = `${raw.nodes.length} thoughts · ${raw.links.length} reasons · ${raw.kerns.length} spheres`
}

function focus(id) {
  focusId = id
  focusLabel.value = id ? (kernsById[id]?.label || 'sphere') : ''
  applyHighlight()
  if (id) {
    const n = objById.get(id)
    if (n && n.x != null) {
      const r = Math.hypot(n.x, n.y, n.z) || 1, d = 90
      G.cameraPosition({ x: n.x * (1 + d / r), y: n.y * (1 + d / r), z: n.z * (1 + d / r) }, n, 800)
    }
  } else {
    G.zoomToFit(700, 60)
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
    .nodeRelSize(3)
    .nodeVal((n) => n._t === 'kern' ? 2 : 1 + (+n.heat || 0) * 2)
    .nodeOpacity(0.9)
    .nodeLabel((n) => n._t === 'kern'
      ? `◉ ${n.label} · ${n.count} thoughts (click to enter)`
      : `${n.kind} · heat ${(+n.heat).toFixed(2)} · conf ${(+n.conf).toFixed(2)}\n${n.label}`)
    .linkVisibility((l) => l._t === 'reason')
    .linkOpacity(0.4)
    .onNodeClick((n) => focus(n._t === 'kern' ? n.id : null))
    .onBackgroundClick(() => focus(null))
  // pull members tightly into their field; loosen reason springs a touch.
  G.d3Force('link').distance((l) => l._t === 'member' ? 14 : l._t === 'hier' ? 60 : 40)
  G.d3Force('charge').strength(-40)
  G.onEngineTick(updateFields)
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
  <div class="hint">click a sphere to step in · drag to orbit · click empty space to zoom out</div>
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
