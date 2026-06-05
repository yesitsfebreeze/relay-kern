<script setup>
import { ref, onMounted, onBeforeUnmount } from 'vue'
import ForceGraph3D from '3d-force-graph'
import * as THREE from 'three'
import SpriteText from 'three-spritetext'

const crumbs = ref([])
const stats = ref('loading…')
const err = ref('')
const graphEl = ref(null)

let G = null
let timer = null
let raw = { nodes: [], links: [], kerns: [] }
let kernsById = {}
let current = null
let levelKey = ''
let wantFit = false
let boundary = null
let boundaryLabel = null

function colorFor(id) {
  let h = 0
  for (let i = 0; i < id.length; i++) h = (h * 31 + id.charCodeAt(i)) >>> 0
  return new THREE.Color(`hsl(${h % 360}, 65%, 62%)`)
}

function rootId() {
  const r = raw.kerns.find(k => !k.parent) || raw.kerns[0]
  return r ? r.id : null
}

function computeCrumbs() {
  const out = []
  let id = current
  while (id && kernsById[id]) {
    out.unshift({ id, label: kernsById[id].named ? kernsById[id].label : '(unnamed)' })
    id = kernsById[id].parent || null
  }
  if (out.length) out[0].label = 'root'
  crumbs.value = out
}

function levelData() {
  const cur = current
  const ents = raw.nodes.filter(n => n.kern === cur)
  const entIds = new Set(ents.map(e => e.id))
  const kids = raw.kerns.filter(k => k.parent === cur)
  const nodes = [
    ...ents.map(e => ({ ...e, _t: 'entity' })),
    ...kids.map(k => ({ id: k.id, label: k.label, named: k.named, count: k.count, _t: 'kern' })),
  ]
  const links = raw.links
    .filter(l => entIds.has(l.source.id ?? l.source) && entIds.has(l.target.id ?? l.target))
    .map(l => ({ source: l.source.id ?? l.source, target: l.target.id ?? l.target }))
  return { nodes, links, ents: ents.length, kids: kids.length }
}

function render(refit) {
  const d = levelData()
  const key = d.nodes.map(n => n.id).sort().join()
  if (key !== levelKey) { levelKey = key; G.graphData({ nodes: d.nodes, links: d.links }); if (refit) wantFit = true }
  const here = kernsById[current]
  stats.value = `${d.ents} thoughts · ${d.kids} sub-spheres` + (here && here.count > d.ents ? ` · ${here.count} total below` : '')
}

function go(id) {
  if (!kernsById[id]) return
  current = id
  computeCrumbs()
  render(true)
}

function up() {
  const p = kernsById[current]?.parent
  if (p && kernsById[p]) go(p)
}

// Distinct geometry per info type so you can read what's what at a glance:
// fact = octahedron, document = cube, question = cone, claim/other = sphere.
// Colour still encodes the owning sphere (cluster).
function entityMesh(n) {
  const s = 2.4 + (+n.heat || 0) * 1.4
  let geo
  switch (n.kind) {
    case 'Fact': geo = new THREE.OctahedronGeometry(s * 1.25); break
    case 'Document': geo = new THREE.BoxGeometry(s * 1.6, s * 1.6, s * 1.6); break
    case 'Question': geo = new THREE.ConeGeometry(s, s * 2, 7); break
    default: geo = new THREE.SphereGeometry(s, 12, 9)
  }
  return new THREE.Mesh(geo, new THREE.MeshLambertMaterial({ color: colorFor(n.kern || n.id) }))
}

function updateBoundary() {
  const ns = (G.graphData().nodes || []).filter(n => n.x != null)
  if (!ns.length) { boundary.visible = false; boundaryLabel.visible = false; return }
  let cx = 0, cy = 0, cz = 0
  for (const n of ns) { cx += n.x; cy += n.y; cz += n.z }
  cx /= ns.length; cy /= ns.length; cz /= ns.length
  let r = 24
  for (const n of ns) r = Math.max(r, Math.hypot(n.x - cx, n.y - cy, n.z - cz))
  r += 16
  boundary.visible = true; boundary.position.set(cx, cy, cz); boundary.scale.set(r, r, r)
  boundaryLabel.visible = true; boundaryLabel.position.set(cx, cy + r + 8, cz)
  const here = crumbs.value[crumbs.value.length - 1]
  boundaryLabel.text = here ? here.label : ''
}

async function load() {
  try {
    raw = await (await fetch('/graph')).json()
    kernsById = {}
    for (const k of raw.kerns) kernsById[k.id] = k
    if (current == null || !kernsById[current]) { current = rootId(); computeCrumbs() }
    render(false)
    err.value = ''
  } catch (e) { err.value = String(e) }
}

onMounted(() => {
  G = ForceGraph3D()(graphEl.value)
    .backgroundColor('#06080b')
    .showNavInfo(false)
    .cooldownTicks(90)
    .nodeLabel((n) => n._t === 'kern'
      ? `◉ ${n.label} · ${n.count} thoughts — click to enter`
      : `${n.kind} · heat ${(+n.heat).toFixed(2)} · conf ${(+n.conf).toFixed(2)}\n${n.label}`)
    .nodeThreeObject((n) => {
      if (n._t === 'kern') {
        const grp = new THREE.Group()
        const r = 6 + Math.cbrt(n.count || 1) * 4
        grp.add(new THREE.Mesh(
          new THREE.SphereGeometry(r, 20, 16),
          new THREE.MeshLambertMaterial({ color: colorFor(n.id), transparent: true, opacity: 0.30, depthWrite: false })
        ))
        const t = new SpriteText(n.named ? n.label : '(unnamed)')
        t.color = colorFor(n.id).getStyle(); t.textHeight = 6; t.position.y = r + 6
        t.material.depthWrite = false
        grp.add(t)
        return grp
      }
      return entityMesh(n)
    })
    .linkColor(() => 'rgba(150,170,190,0.30)')
    .linkOpacity(0.35)
    .onNodeClick((n) => { if (n._t === 'kern') go(n.id) })
    .onBackgroundClick(() => up())
    .onEngineStop(() => { if (wantFit) { G.zoomToFit(700, 70); wantFit = false } })

  // Faint enclosing field: the sphere you are currently inside. Gives spatial
  // context without adding clutter — a ghost wireframe + a big dim label.
  boundary = new THREE.Mesh(
    new THREE.SphereGeometry(1, 28, 20),
    new THREE.MeshBasicMaterial({ color: 0x3a4a64, wireframe: true, transparent: true, opacity: 0.06 })
  )
  G.scene().add(boundary)
  boundaryLabel = new SpriteText('')
  boundaryLabel.color = 'rgba(120,140,170,0.35)'; boundaryLabel.textHeight = 11
  boundaryLabel.material.depthWrite = false
  G.scene().add(boundaryLabel)
  G.onEngineTick(updateBoundary)

  load()
  timer = setInterval(load, 5000)
})

onBeforeUnmount(() => { if (timer) clearInterval(timer) })
</script>

<template>
  <div class="hud">
    <b>kern</b>
    <span class="crumbs">
      <template v-for="(c, i) in crumbs" :key="c.id">
        <a @click="go(c.id)" :class="{ here: i === crumbs.length - 1 }">{{ c.label }}</a>
        <span v-if="i < crumbs.length - 1" class="sep"> › </span>
      </template>
    </span>
    <span class="stat">· {{ stats }}</span>
    <span v-if="err" class="err"> — {{ err }}</span>
  </div>
  <div class="legend">
    <span>● claim</span><span class="d">◆ fact</span><span class="d">■ document</span>
    <span class="d">▲ question</span><span class="d">◉ sub-sphere</span>
    <span class="dim">· color = sphere · size = heat</span>
  </div>
  <div class="hint">click a sphere to step in · click empty space to go up · drag to orbit</div>
  <div ref="graphEl" class="graph"></div>
</template>

<style>
.graph { width: 100vw; height: 100vh; }
.hud {
  position: fixed; top: 10px; left: 12px; right: 12px; z-index: 10;
  background: #11151aee; color: #cdd3da; padding: 8px 12px; border-radius: 8px;
  border: 1px solid #222a33; font: 13px system-ui, sans-serif;
  display: flex; gap: 8px; align-items: center; flex-wrap: wrap;
}
.hud b { color: #7fd1ae; }
.crumbs a { color: #8fb6d8; cursor: pointer; }
.crumbs a.here { color: #e5c07b; font-weight: 600; }
.crumbs .sep { color: #4a5563; }
.stat { color: #9aa6b2; }
.err { color: #e06c75; }
.legend {
  position: fixed; top: 52px; left: 12px; z-index: 10; color: #9aa6b2;
  font: 12px system-ui, sans-serif; background: #11151acc; padding: 4px 10px;
  border-radius: 6px; display: flex; gap: 12px;
}
.legend .d { color: #cdd3da; }
.legend .dim { color: #5a6573; }
.hint { position: fixed; bottom: 10px; left: 12px; z-index: 10; color: #5a6573; font: 12px system-ui, sans-serif; }
</style>
