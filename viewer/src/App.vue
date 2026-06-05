<script setup>
import { ref, onMounted, onBeforeUnmount } from 'vue'
import * as d3 from 'd3'

const crumbs = ref([])
const stats = ref('loading…')
const err = ref('')
const detail = ref('')
const mode = ref('cube')      // 'cube' | 'list'
const tiles = ref([])
const listItems = ref([])
const side = ref(480)
const zoomDir = ref('in')
const levelId = ref('')
const stageEl = ref(null)

let timer = null
let raw = { nodes: [], links: [], kerns: [] }
let kernsById = {}, entsByKern = new Map(), meanHeat = {}
let treeData = null
let stack = []
let lastTopo = ''

const KIND = { Fact: '#e5c07b', Document: '#61afef', Question: '#c678dd', Claim: '#98c379' }
const MARK = { Fact: '◆', Document: '■', Question: '▲', Claim: '●' }

function rootId() { const r = raw.kerns.find(k => !k.parent) || raw.kerns[0]; return r ? r.id : null }
function buildTree() {
  const make = (kid, seen) => {
    const k = kernsById[kid]; if (!k || seen.has(kid)) return null
    seen.add(kid)
    const node = { id: kid, label: k.named ? k.label : '(unnamed)', type: 'kern', children: [] }
    for (const c of k.children || []) { const cn = make(c, seen); if (cn) node.children.push(cn) }
    for (const e of entsByKern.get(kid) || [])
      node.children.push({ id: e.id, label: e.label, type: 'entity', kind: e.kind, heat: e.heat, conf: e.conf })
    return node
  }
  return make(rootId(), new Set()) || { id: 'root', label: 'root', type: 'kern', children: [] }
}
function d3Count(d) { if (d.type === 'entity') return 1; let n = 0; const w = x => { if (x.type === 'entity') n++; else for (const c of x.children || []) w(c) }; w(d); return n || 1 }
function subSpheres(d) { return (d.children || []).filter(c => c.type === 'kern').length }
function meanHeatOf(node) { let s = 0, n = 0; const w = x => { if (x.type === 'entity') { s += +x.heat || 0; n++ } else for (const c of x.children || []) w(c) }; w(node); return n ? s / n : 0 }
function findPath(node, id, acc = []) { acc.push(node); if (node.id === id) return acc.slice(); for (const c of node.children || []) { const r = findPath(c, id, acc); if (r) return r } acc.pop(); return null }
function findById(n, id) { if (n.id === id) return n; for (const c of n.children || []) { const r = findById(c, id); if (r) return r } return null }

const heatMax = () => Math.max(0.5, d3.max(raw.nodes, n => +n.heat || 0) || 1)
// refined warm ramp (ember → amber → cream); no muddy purple/black.
const WARM = d3.interpolateRgbBasis(['#2a1809', '#7c3c17', '#cf6f25', '#f2a93e', '#ffe2a6'])
function ramp(h) { return WARM(0.12 + 0.85 * Math.sqrt(Math.min(1, (h || 0) / heatMax()))) }
function fill(ref) { return ramp(ref.type === 'entity' ? ref.heat : meanHeat[ref.id]) }
function textColor(ref) {
  const c = d3.color(fill(ref)); if (!c) return '#fff'
  const lum = (0.299 * c.r + 0.587 * c.g + 0.114 * c.b) / 255
  return lum > 0.62 ? '#1c1206' : '#fdfaf3'
}
function meta(ref) { return ref.type === 'kern' ? `${d3Count(ref)} thoughts${subSpheres(ref) ? ` · ${subSpheres(ref)} spheres` : ''}` : ref.kind }
function info(ref) { return ref.type === 'entity' ? `${ref.kind} · heat ${(+ref.heat).toFixed(2)} · conf ${(+ref.conf).toFixed(2)} — ${ref.label}` : `${ref.label} · ${d3Count(ref)} thoughts — click to enter` }

function relayout() {
  const cur = stack[stack.length - 1]
  levelId.value = cur.id
  crumbs.value = stack.map((n, i) => ({ id: n.id, label: i === 0 ? 'root' : n.label }))
  const kids = cur.children || []
  const kernKids = kids.filter(c => c.type === 'kern')

  // Last level (no sub-spheres) → a scrollable list of thoughts, not a blob.
  if (kernKids.length === 0) {
    mode.value = 'list'
    listItems.value = kids.filter(c => c.type === 'entity').sort((a, b) => (b.heat || 0) - (a.heat || 0))
    stats.value = `${raw.nodes.length} thoughts · ${raw.kerns.length} spheres · here: ${listItems.value.length}`
    return
  }

  // Otherwise → a framed square holding a bento grid of readable cards. Cards
  // never shrink below a min size: bigger topics span more cells, and if there
  // are more than fit, the square scrolls. (No tiny unreadable tiles.)
  mode.value = 'cube'
  const s = Math.round(Math.min(stageEl.value.clientWidth, stageEl.value.clientHeight, 820) * 0.66)
  side.value = Math.max(300, Math.min(s, 660))
  const counted = kids.map(c => ({ ref: c, count: d3Count(c) })).sort((a, b) => b.count - a.count)
  const max = counted[0]?.count || 1
  tiles.value = counted.map((t, i) => ({
    ...t, i,
    span: t.ref.type === 'entity' ? 'sm' : (t.count >= max * 0.5 ? 'lg' : t.count >= max * 0.18 ? 'md' : 'sm'),
  }))
  stats.value = `${raw.nodes.length} thoughts · ${raw.kerns.length} spheres · here: ${kids.length} topics`
}

function enter(ref) { if (ref.type !== 'kern') return; const p = findPath(treeData, ref.id); if (p) { zoomDir.value = 'in'; stack = p; relayout() } }
function out() { if (stack.length > 1) { zoomDir.value = 'out'; stack.pop(); relayout() } }
function goTo(id) { const i = stack.findIndex(n => n.id === id); if (i >= 0) { zoomDir.value = 'out'; stack.length = i + 1; relayout() } }
function onKey(ev) { if (ev.key === 'Escape') { ev.preventDefault(); out() } }

async function load() {
  try {
    raw = await (await fetch('/graph')).json()
    kernsById = {}; entsByKern = new Map()
    for (const k of raw.kerns) kernsById[k.id] = k
    for (const e of raw.nodes) { if (!entsByKern.has(e.kern)) entsByKern.set(e.kern, []); entsByKern.get(e.kern).push(e) }
    const topo = raw.nodes.length + ':' + raw.kerns.length
    if (topo !== lastTopo) {
      lastTopo = topo; treeData = buildTree()
      meanHeat = {}; const reg = x => { if (x.type === 'kern') { meanHeat[x.id] = meanHeatOf(x); for (const c of x.children || []) reg(c) } }; reg(treeData)
      const ids = stack.map(n => n.id); stack = [treeData]
      for (let i = 1; i < ids.length; i++) { const n = findById(treeData, ids[i]); if (n) stack.push(n); else break }
      relayout()
    }
    err.value = ''
  } catch (e) { err.value = String(e) }
}

onMounted(() => {
  window.addEventListener('keydown', onKey)
  window.addEventListener('resize', () => relayout())
  load(); timer = setInterval(load, 5000)
})
onBeforeUnmount(() => { if (timer) clearInterval(timer); window.removeEventListener('keydown', onKey) })
</script>

<template>
  <div class="hud">
    <b>kern</b>
    <span class="crumbs">
      <template v-for="(c, i) in crumbs" :key="c.id">
        <a @click="goTo(c.id)" :class="{ here: i === crumbs.length - 1 }">{{ c.label }}</a>
        <span v-if="i < crumbs.length - 1" class="sep"> › </span>
      </template>
    </span>
    <span class="stat">· {{ stats }}</span>
    <span v-if="err" class="err"> — {{ err }}</span>
  </div>

  <div ref="stageEl" class="stage">
    <div v-if="mode === 'cube'" class="frame" :style="{ width: side + 'px', height: side + 'px' }">
      <div class="grid" :class="'z-' + zoomDir" :key="levelId">
        <div v-for="t in tiles" :key="t.ref.id" class="card" :class="[t.ref.type, t.span]"
          :style="{ background: fill(t.ref), color: textColor(t.ref) }"
          @click="enter(t.ref)" @mouseenter="detail = info(t.ref)" @mouseleave="detail = ''">
          <div class="cname">{{ t.ref.label }}</div>
          <div class="cmeta">{{ meta(t.ref) }}</div>
        </div>
      </div>
    </div>

    <div v-else class="list">
      <div class="lhead">{{ listItems.length }} thoughts in this sphere</div>
      <div v-for="e in listItems" :key="e.id" class="row"
        @mouseenter="detail = info(e)" @mouseleave="detail = ''">
        <span class="rk" :style="{ color: KIND[e.kind] || '#98c379' }">{{ MARK[e.kind] || '·' }}</span>
        <span class="rt">{{ e.label }}</span>
        <span class="rbar"><i :style="{ width: Math.min(100, (e.heat / heatMax()) * 100) + '%', background: fill(e) }"></i></span>
      </div>
    </div>
  </div>

  <div class="path">{{ detail || 'click a cube to enter · Esc to go back' }}</div>
</template>

<style>
:root {
  --bg: #0a0a0c;
  --ink: #f4f1ea;
  --muted: #8b8678;
  --line: rgba(244,241,234,0.10);
  --display: 'Bricolage Grotesque', system-ui, sans-serif;
  --body: 'Hanken Grotesk', system-ui, sans-serif;
  --mono: 'IBM Plex Mono', ui-monospace, monospace;
}
* { box-sizing: border-box; }
html, body, #app { height: 100%; margin: 0; }

.stage {
  position: fixed; inset: 0; display: flex; align-items: center; justify-content: center;
  padding: 96px 64px 84px;
  background:
    radial-gradient(120% 90% at 50% -10%, #16130f 0%, #0a0a0c 55%, #08080a 100%);
}

.frame { overflow-y: auto; overflow-x: hidden; border-radius: 20px;
  box-shadow: inset 0 0 0 1px rgba(244,241,234,0.06); padding: 14px;
  scrollbar-width: thin; scrollbar-color: #26241f transparent; }
.frame::-webkit-scrollbar { width: 8px; } .frame::-webkit-scrollbar-thumb { background: #26241f; border-radius: 4px; }

.grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
  grid-auto-rows: 150px; grid-auto-flow: dense; gap: 14px; min-height: 100%; }
.grid.z-in { animation: zin .4s cubic-bezier(.2,.8,.2,1); }
.grid.z-out { animation: zout .4s cubic-bezier(.2,.8,.2,1); }
@keyframes zin { from { opacity: 0; transform: scale(.86); } to { opacity: 1; transform: scale(1); } }
@keyframes zout { from { opacity: 0; transform: scale(1.12); } to { opacity: 1; transform: scale(1); } }

.card { border-radius: 16px; padding: 16px; overflow: hidden;
  display: flex; flex-direction: column; justify-content: flex-end; gap: 8px;
  box-shadow: inset 0 0 0 1px rgba(255,255,255,0.08), 0 10px 28px -16px rgba(0,0,0,0.75);
  cursor: pointer; transition: transform .18s cubic-bezier(.2,.7,.2,1), filter .18s, box-shadow .18s; }
.card.lg { grid-column: span 2; grid-row: span 2; }
.card.md { grid-column: span 2; }
.card.entity { cursor: default; }
.card.kern:hover { transform: translateY(-4px); filter: brightness(1.1);
  box-shadow: inset 0 0 0 1px rgba(255,255,255,0.55), 0 22px 44px -18px rgba(0,0,0,0.85); }
.cname { font-family: var(--display); font-weight: 800; line-height: 1.06; letter-spacing: -0.02em;
  color: inherit; font-size: 17px;
  display: -webkit-box; -webkit-line-clamp: 4; -webkit-box-orient: vertical; overflow: hidden; }
.card.lg .cname { font-size: 26px; -webkit-line-clamp: 5; }
.card.md .cname { font-size: 20px; }
.cmeta { font-family: var(--mono); font-size: 10px; letter-spacing: .14em; text-transform: uppercase;
  color: inherit; opacity: .66; }

/* leaf list — editorial, roomy, readable */
.list { width: min(720px, 90vw); height: calc(100vh - 200px); overflow-y: auto; padding: 4px 2px 40px; }
.lhead { font-family: var(--mono); font-size: 11px; letter-spacing: .18em; text-transform: uppercase;
  color: var(--muted); padding: 2px 4px 18px; }
.row { display: flex; align-items: center; gap: 16px; padding: 15px 4px; border-bottom: 1px solid var(--line); }
.row:hover { background: rgba(244,241,234,0.02); }
.rk { font-size: 13px; flex: none; width: 16px; text-align: center; }
.rt { flex: 1; color: var(--ink); font-family: var(--body); font-size: 15px; line-height: 1.5; font-weight: 500; }
.rbar { flex: none; width: 64px; height: 4px; background: rgba(244,241,234,0.08); border-radius: 3px; overflow: hidden; }
.rbar i { display: block; height: 100%; border-radius: 3px; }

.hud { position: fixed; top: 26px; left: 32px; z-index: 10; color: var(--ink);
  font-family: var(--body); font-size: 13px; display: flex; gap: 10px; align-items: baseline; flex-wrap: wrap; max-width: 88vw; }
.hud b { font-family: var(--display); font-weight: 800; font-size: 17px; letter-spacing: -0.02em; color: var(--ink); }
.crumbs a { color: var(--muted); cursor: pointer; transition: color .15s; }
.crumbs a:hover { color: var(--ink); }
.crumbs a.here { color: #f2a93e; font-weight: 600; }
.crumbs .sep { color: #3a3630; margin: 0 2px; }
.stat { font-family: var(--mono); font-size: 11px; letter-spacing: .06em; color: var(--muted); }
.err { color: #e8705e; }
.path { position: fixed; bottom: 26px; left: 32px; right: 32px; z-index: 10; color: var(--ink);
  font-family: var(--body); font-size: 14px; font-weight: 500;
  white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.list::-webkit-scrollbar { width: 8px; } .list::-webkit-scrollbar-thumb { background: #1d1c19; border-radius: 4px; }
</style>
