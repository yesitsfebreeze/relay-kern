<script setup>
import { ref, onMounted, onBeforeUnmount, nextTick } from 'vue'

const KIND = { Fact: '#e5c07b', Document: '#61afef', Question: '#c678dd', Claim: '#98c379' }
const MARK = { Fact: '◆', Document: '■', Question: '▲', Claim: '●' }

const stats = ref('')
const err = ref('')
const turns = ref([])      // {role:'user'|'oracle', text, sources?, chains?, reasons?}
const sources = ref([])    // current answer's source tiles (global ref kept for convenience)
const chains = ref([])     // current answer's provenance strings
const reasons = ref([])    // current answer's reason edges (structured)
const input = ref('')
const busy = ref(false)
const hot = ref(null)      // hovered/active citation number
const inputEl = ref(null)
const scrollEl = ref(null)

const editing = ref(null)  // {id, kind, text} of the item currently being edited
const saved = ref(new Set()) // ids that were just corrected

let history = []           // [{role, content}] sent to the server
let ctrl = null            // AbortController for the in-flight stream
let pulse = null

const heatMax = ref(1)
function ramp(h) {
  const t = Math.min(1, Math.sqrt((h || 0) / (heatMax.value || 1)))
  const lo = [42, 24, 9], hi = [255, 226, 166]
  const c = lo.map((v, i) => Math.round(v + (hi[i] - v) * (0.12 + 0.85 * t)))
  return `rgb(${c[0]},${c[1]},${c[2]})`
}
function textColor(bg) {
  const m = bg.match(/\d+/g) || [0, 0, 0]
  return (0.299 * m[0] + 0.587 * m[1] + 0.114 * m[2]) / 255 > 0.62 ? '#1c1206' : '#fdfaf3'
}

// Split an answer into text + [n] citation chips for rendering.
function segments(text) {
  const out = []
  const re = /\[(\d+)\]/g
  let last = 0, m
  while ((m = re.exec(text))) {
    if (m.index > last) out.push({ t: text.slice(last, m.index) })
    out.push({ cite: +m[1] })
    last = m.index + m[0].length
  }
  if (last < text.length) out.push({ t: text.slice(last) })
  return out
}

async function loadStats() {
  try {
    const g = await (await fetch('/graph')).json()
    const groups = (g.kerns || []).filter(k => k.id !== '__all__').length
    heatMax.value = Math.max(1, ...(g.nodes || []).map(n => +n.heat || 0))
    stats.value = `${(g.nodes || []).length} thoughts · ${groups} groups`
    err.value = ''
  } catch (e) { err.value = String(e) }
}

async function ask() {
  const q = input.value.trim()
  if (!q || busy.value) return
  if (ctrl) ctrl.abort()
  ctrl = new AbortController()
  input.value = ''
  busy.value = true
  sources.value = []; chains.value = []; reasons.value = []; hot.value = null
  editing.value = null
  turns.value.push({ role: 'user', text: q })
  turns.value.push({ role: 'oracle', text: '', sources: [], chains: [], reasons: [] })
  const oracle = turns.value[turns.value.length - 1]
  await scrollDown()
  try {
    const res = await fetch('/ask', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ question: q, history: history.slice(-6) }),
      signal: ctrl.signal,
    })
    if (!res.ok || !res.body) throw new Error('oracle unavailable')
    const reader = res.body.getReader()
    const dec = new TextDecoder()
    let buf = ''
    for (;;) {
      const { value, done } = await reader.read()
      if (done) break
      buf += dec.decode(value, { stream: true })
      let i
      while ((i = buf.indexOf('\n\n')) >= 0) {
        const frame = buf.slice(0, i); buf = buf.slice(i + 2)
        handleFrame(frame, oracle)
      }
    }
    history.push({ role: 'user', content: q })
    history.push({ role: 'assistant', content: oracle.text })
  } catch (e) {
    if (e.name !== 'AbortError') oracle.text = oracle.text || '⚠ oracle unavailable'
  } finally {
    busy.value = false
    await scrollDown()
  }
}

// Parse one SSE frame ("event: x\ndata: {json}") and apply it.
function handleFrame(frame, oracle) {
  let ev = 'message', data = ''
  for (const line of frame.split('\n')) {
    if (line.startsWith('event:')) ev = line.slice(6).trim()
    else if (line.startsWith('data:')) data += (data ? '\n' : '') + line.slice(5).trim()
  }
  let d = {}
  try { d = data ? JSON.parse(data) : {} } catch (_) { return }
  if (ev === 'sources') {
    sources.value = d.entities || []
    chains.value = d.chains || []
    reasons.value = d.reasons || []
    oracle.sources = sources.value; oracle.chains = chains.value; oracle.reasons = reasons.value
    scrollDown()
  } else if (ev === 'token') {
    oracle.text += d.t || ''
    scrollDown()
  } else if (ev === 'error') {
    oracle.text = (oracle.text || '') + `\n⚠ ${d.message || 'oracle error'}`
  }
}

async function scrollDown() {
  await nextTick()
  const el = scrollEl.value
  if (el) el.scrollTop = el.scrollHeight
}

function onKey(ev) {
  if (ev.key === 'Enter' && !ev.shiftKey) { ev.preventDefault(); ask() }
}

function startEdit(item, kind) {
  editing.value = { id: item.id, kind, text: item.text || item.label || '' }
}

function cancelEdit() {
  editing.value = null
}

async function saveEdit(id, text, kind) {
  editing.value = null
  try {
    const res = await fetch('/edit', {
      method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ id, text, kind }),
    })
    const d = await res.json().catch(() => ({}))
    if (d && d.ok) {
      const newSaved = new Set(saved.value)
      newSaved.add(id)
      saved.value = newSaved
      // reflect the correction locally in the global refs
      const hit = [...sources.value, ...reasons.value].find(x => x.id === id)
      if (hit) { hit.text = text; if ('label' in hit) hit.label = text.slice(0, 80) }
      // also patch within per-turn arrays so each turn's side panels update immediately
      for (const turn of turns.value) {
        if (turn.sources) {
          const s = turn.sources.find(x => x.id === id)
          if (s) { s.text = text; if ('label' in s) s.label = text.slice(0, 80) }
        }
        if (turn.reasons) {
          const r = turn.reasons.find(x => x.id === id)
          if (r) { r.text = text }
        }
      }
    }
  } catch (_) { /* keep UI as-is on failure */ }
}

onMounted(() => {
  loadStats(); pulse = setInterval(loadStats, 5000)
  inputEl.value?.focus()
})
onBeforeUnmount(() => { if (pulse) clearInterval(pulse); if (ctrl) ctrl.abort() })
</script>

<template>
  <div class="app">
    <header class="rail">
      <div class="brand"><b>kern</b><span class="sub">oracle</span></div>
      <div class="rstats"><span class="dot"></span>{{ stats }}<span v-if="err" class="err"> — {{ err }}</span></div>
    </header>

    <div class="miller-scroll" ref="scrollEl">
      <div v-if="!turns.length" class="hint-wrap">
        <div class="hint">Ask the oracle anything about your memory.</div>
      </div>

      <!-- 3-column grid; each oracle turn emits 3 sibling cells via display:contents wrapper -->
      <div class="miller-grid">
        <template v-for="(t, i) in turns" :key="i">
          <!-- USER TURN: left empty · center bubble · right empty -->
          <template v-if="t.role === 'user'">
            <div class="mc-left mc-user-empty"></div>
            <div class="mc-center">
              <div class="turn user">
                <div class="ubody">{{ t.text }}</div>
              </div>
            </div>
            <div class="mc-right mc-user-empty"></div>
          </template>

          <!-- ORACLE TURN: left=sources · center=answer · right=reasons -->
          <template v-else>
            <!-- LEFT: incoming memories (sources) -->
            <div class="mc-left mc-sticky">
              <div v-if="t.sources && t.sources.length" class="side-panel incoming">
                <div class="side-head">
                  <span class="side-label">incoming</span>
                  <span class="side-count">{{ t.sources.length }}</span>
                </div>
                <div class="src-list">
                  <div
                    v-for="s in t.sources"
                    :key="s.id"
                    class="tile"
                    :class="{ on: hot === s.n, corrected: saved.has(s.id) }"
                    :style="{ background: ramp(s.heat), color: textColor(ramp(s.heat)) }"
                    @mouseenter="hot = s.n"
                    @mouseleave="hot = null"
                  >
                    <span class="tn">{{ s.n }}</span>
                    <span class="tmark">{{ MARK[s.kind] || '·' }}</span>
                    <template v-if="editing && editing.id === s.id">
                      <textarea
                        class="edit-area"
                        v-model="editing.text"
                        @keydown.enter.ctrl="saveEdit(editing.id, editing.text, 'entity')"
                        @keydown.esc="cancelEdit"
                        rows="4"
                        autofocus
                      ></textarea>
                      <div class="edit-actions">
                        <button class="ebtn save" @click.stop="saveEdit(editing.id, editing.text, 'entity')">save</button>
                        <button class="ebtn" @click.stop="cancelEdit">cancel</button>
                      </div>
                    </template>
                    <template v-else>
                      <div class="tname">{{ s.label }}</div>
                      <div class="tmeta">{{ s.kind }} · {{ (+s.score).toFixed(2) }}</div>
                      <button class="edit-btn" title="edit thought" @click.stop="startEdit(s, 'entity')">✎</button>
                      <span v-if="saved.has(s.id)" class="badge-ok">✓ corrected · reevaluating</span>
                    </template>
                  </div>
                </div>
              </div>
              <div v-else class="mc-empty-side"></div>
            </div>

            <!-- CENTER: oracle answer bubble -->
            <div class="mc-center">
              <div class="turn oracle">
                <span class="oglyph">◈</span>
                <div class="obody">
                  <span v-for="(s, j) in segments(t.text)" :key="j">
                    <span
                      v-if="s.cite"
                      class="cite"
                      :class="{ on: hot === s.cite }"
                      @mouseenter="hot = s.cite"
                      @mouseleave="hot = null"
                    >{{ s.cite }}</span>
                    <span v-else>{{ s.t }}</span>
                  </span>
                  <span v-if="busy && i === turns.length - 1" class="caret">▍</span>
                </div>
              </div>
            </div>

            <!-- RIGHT: outgoing reasoning (reason edges) -->
            <div class="mc-right mc-sticky">
              <div v-if="t.reasons && t.reasons.length" class="side-panel outgoing">
                <div class="side-head">
                  <span class="side-label">reasoning</span>
                  <span class="side-count">{{ t.reasons.length }}</span>
                </div>
                <div
                  v-for="r in t.reasons"
                  :key="r.id"
                  class="reason-row"
                  :class="{ corrected: saved.has(r.id) }"
                >
                  <template v-if="editing && editing.id === r.id">
                    <textarea
                      class="edit-area reason-edit-area"
                      v-model="editing.text"
                      @keydown.enter.ctrl="saveEdit(editing.id, editing.text, 'reason')"
                      @keydown.esc="cancelEdit"
                      rows="3"
                      autofocus
                    ></textarea>
                    <div class="edit-actions">
                      <button class="ebtn save" @click.stop="saveEdit(editing.id, editing.text, 'reason')">save</button>
                      <button class="ebtn" @click.stop="cancelEdit">cancel</button>
                    </div>
                  </template>
                  <template v-else>
                    <span class="reason-kind">{{ r.kind }}</span>
                    <span class="reason-text">{{ r.text }}</span>
                    <div class="reason-row-actions">
                      <button class="edit-btn" title="edit reason" @click.stop="startEdit(r, 'reason')">✎</button>
                      <span v-if="saved.has(r.id)" class="badge-ok">✓ corrected · reevaluating</span>
                    </div>
                  </template>
                </div>
              </div>
              <div v-else class="mc-empty-side"></div>
            </div>
          </template>
        </template>
      </div>

      <!-- Input bar pinned to bottom of center column via a full-width footer row -->
      <div class="ask-footer">
        <div class="ask-footer-inner">
          <input
            ref="inputEl"
            v-model="input"
            @keydown="onKey"
            :disabled="busy"
            placeholder="ask the oracle…"
          />
          <button @click="ask" :disabled="busy || !input.trim()">↵</button>
        </div>
      </div>
    </div>
  </div>
</template>

<style>
:root {
  --ink: #f4f1ea;
  --muted: #8b8678;
  --line: rgba(244,241,234,0.10);
  --panel: rgba(244,241,234,0.018);
  --rail-h: 52px;
  --display: 'Bricolage Grotesque', system-ui, sans-serif;
  --body: 'Hanken Grotesk', system-ui, sans-serif;
  --mono: 'IBM Plex Mono', ui-monospace, monospace;
}
* { box-sizing: border-box; }
html, body, #app { height: 100%; margin: 0; }

.app {
  height: 100%;
  display: flex;
  flex-direction: column;
  background: radial-gradient(120% 90% at 50% -10%, #16130f 0%, #0a0a0c 55%, #08080a 100%);
  color: var(--ink);
  font-family: var(--body);
}

/* ── rail ── */
.rail {
  height: var(--rail-h);
  display: flex;
  align-items: baseline;
  gap: 14px;
  padding: 14px 22px;
  border-bottom: 1px solid var(--line);
  flex-shrink: 0;
}
.brand b { font-family: var(--display); font-weight: 800; font-size: 17px; }
.brand .sub { color: var(--muted); font-family: var(--mono); font-size: 11px; letter-spacing: .16em; text-transform: uppercase; margin-left: 6px; }
.rstats { color: var(--muted); font-family: var(--mono); font-size: 11px; display: flex; align-items: center; gap: 8px; }
.dot { width: 7px; height: 7px; border-radius: 50%; background: #98c379; box-shadow: 0 0 8px #98c379; }
.err { color: #e8705e; }

/* ── outer scroll container ── */
.miller-scroll {
  flex: 1;
  min-height: 0;
  overflow-y: auto;
  display: flex;
  flex-direction: column;
}

/* ── hint (empty state) ── */
.hint-wrap {
  flex: 1;
  display: flex;
  align-items: center;
  justify-content: center;
}
.hint { color: var(--muted); font-size: 14px; }

/* ── miller 3-column grid ── */
.miller-grid {
  flex: 1;
  display: grid;
  grid-template-columns: minmax(220px, 1fr) minmax(380px, 1.5fr) minmax(240px, 1fr);
  column-gap: 0;
  row-gap: 0;
  align-items: start;
  padding: 18px 0 0;
}

/* column dividers */
.miller-grid::before,
.miller-grid::after {
  display: none; /* handled via cell borders */
}

/* ── grid cells ── */
.mc-left,
.mc-center,
.mc-right {
  padding: 0 16px 24px;
}
.mc-left {
  border-right: 1px solid var(--line);
}
.mc-right {
  border-left: 1px solid var(--line);
}

/* sticky side panels pin to top of viewport below the rail */
.mc-sticky {
  position: sticky;
  top: var(--rail-h);
  align-self: start;
}

.mc-user-empty,
.mc-empty-side {
  /* intentionally blank */
}

/* ── center column turns ── */
.turn.user {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 4px;
}
.ubody {
  background: rgba(242,169,62,0.14);
  color: var(--ink);
  padding: 9px 13px;
  border-radius: 13px 13px 3px 13px;
  font-size: 14px;
  max-width: 88%;
}
.turn.oracle {
  display: flex;
  gap: 10px;
}
.oglyph { color: #98c379; flex-shrink: 0; padding-top: 2px; }
.obody { font-size: 15px; line-height: 1.5; }
.cite {
  display: inline-flex;
  align-items: center;
  justify-content: center;
  min-width: 17px;
  height: 17px;
  padding: 0 4px;
  margin: 0 1px;
  border-radius: 5px;
  background: rgba(97,175,239,0.18);
  color: #9cd0ff;
  font-family: var(--mono);
  font-size: 11px;
  cursor: pointer;
  vertical-align: 1px;
}
.cite.on { background: #61afef; color: #08080a; }
.caret { color: #f2a93e; animation: blink 1s steps(2) infinite; }
@keyframes blink { 50% { opacity: 0; } }

/* ── side panels (incoming / outgoing) ── */
.side-panel {
  border-radius: 14px;
  background: var(--panel);
  box-shadow: inset 0 0 0 1px rgba(244,241,234,0.06);
  overflow: hidden;
}
.side-head {
  font-family: var(--mono);
  font-size: 10px;
  letter-spacing: .16em;
  text-transform: uppercase;
  padding: 10px 13px;
  border-bottom: 1px solid var(--line);
  display: flex;
  align-items: center;
  gap: 8px;
  color: var(--muted);
}
.side-label { color: var(--ink); opacity: .7; }
.side-count { margin-left: auto; }

/* ── source tiles (left column) ── */
.src-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
  padding: 10px;
}
.tile {
  position: relative;
  border-radius: 11px;
  padding: 10px 10px 10px 10px;
  min-height: 80px;
  display: flex;
  flex-direction: column;
  justify-content: flex-end;
  gap: 4px;
  box-shadow: inset 0 0 0 1px rgba(255,255,255,0.08);
  transition: box-shadow .12s;
}
.tile.on { box-shadow: inset 0 0 0 2px #61afef, 0 0 0 2px rgba(97,175,239,0.4); }
.tile.corrected { box-shadow: inset 0 0 0 1px rgba(152,195,121,0.45); }
.tn { position: absolute; top: 8px; left: 10px; font-family: var(--mono); font-size: 10px; opacity: .8; }
.tmark { position: absolute; top: 7px; right: 10px; opacity: .8; }
.tname { font-family: var(--display); font-weight: 800; font-size: 13px; line-height: 1.15; display: -webkit-box; -webkit-line-clamp: 3; -webkit-box-orient: vertical; overflow: hidden; }
.tmeta { font-family: var(--mono); font-size: 10px; opacity: .75; }

/* ── reason rows (right column) ── */
.reason-row {
  position: relative;
  padding: 8px 10px;
  border-radius: 9px;
  background: rgba(244,241,234,0.03);
  box-shadow: inset 0 0 0 1px var(--line);
  margin: 8px 10px 0;
  display: flex;
  flex-direction: column;
  gap: 3px;
}
.reason-row:last-child { margin-bottom: 10px; }
.reason-row.corrected { box-shadow: inset 0 0 0 1px rgba(152,195,121,0.35); }
.reason-kind { font-family: var(--mono); font-size: 10px; color: var(--muted); text-transform: uppercase; letter-spacing: .1em; }
.reason-text { font-size: 13px; line-height: 1.4; color: var(--ink); padding-right: 26px; }
.reason-row-actions { display: flex; align-items: center; gap: 8px; margin-top: 2px; }
.reason-edit-area { margin-top: 4px; }

/* ── edit affordances ── */
.edit-btn {
  position: absolute;
  bottom: 7px;
  right: 8px;
  background: rgba(244,241,234,0.12);
  border: none;
  border-radius: 5px;
  padding: 2px 5px;
  font-size: 12px;
  cursor: pointer;
  color: inherit;
  opacity: 0;
  transition: opacity .12s;
}
.tile:hover .edit-btn, .reason-row:hover .edit-btn { opacity: 1; }
.edit-area {
  width: 100%;
  background: rgba(0,0,0,0.3);
  border: 1px solid rgba(244,241,234,0.2);
  border-radius: 7px;
  color: var(--ink);
  font-family: var(--body);
  font-size: 13px;
  padding: 7px;
  resize: vertical;
  outline: none;
}
.edit-actions { display: flex; gap: 6px; margin-top: 4px; }
.ebtn {
  background: rgba(244,241,234,0.10);
  border: 1px solid rgba(244,241,234,0.15);
  border-radius: 6px;
  color: var(--ink);
  font-size: 11px;
  padding: 3px 8px;
  cursor: pointer;
}
.ebtn.save { background: rgba(152,195,121,0.22); border-color: rgba(152,195,121,0.4); color: #c8efae; }
.badge-ok { font-family: var(--mono); font-size: 10px; color: #98c379; opacity: .8; margin-top: 2px; display: block; }

/* ── ask footer ── */
.ask-footer {
  position: sticky;
  bottom: 0;
  background: linear-gradient(to top, #0a0a0c 70%, transparent);
  padding: 10px 0 16px;
  /* align the input to the center column */
  display: grid;
  grid-template-columns: minmax(220px, 1fr) minmax(380px, 1.5fr) minmax(240px, 1fr);
}
.ask-footer-inner {
  grid-column: 2;
  display: flex;
  gap: 10px;
  padding: 0 16px;
}
.ask-footer-inner input {
  flex: 1;
  background: #131210;
  border: 0;
  outline: none;
  color: var(--ink);
  font-family: var(--body);
  font-size: 14px;
  padding: 13px 15px;
  border-radius: 11px;
  box-shadow: inset 0 0 0 1px var(--line);
}
.ask-footer-inner input::placeholder { color: #4f4b43; }
.ask-footer-inner button {
  background: #f2a93e;
  color: #1c1206;
  border: 0;
  border-radius: 11px;
  width: 46px;
  font-size: 16px;
  cursor: pointer;
}
.ask-footer-inner button:disabled { opacity: .4; cursor: default; }
</style>
