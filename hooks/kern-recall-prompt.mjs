#!/usr/bin/env node
// Claude Code UserPromptSubmit hook: demand-driven semantic recall.
// On every prompt, run `kern search <prompt>` against the cwd's `.kern`
// store and inject the top scored thoughts as additionalContext. Fail-open:
// any error, timeout, or missing store exits 0 with no output.
import fs from 'node:fs';
import path from 'node:path';
import { execFile } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// ── tunables ──────────────────────────────────────────────────────────────
export const TOP_K        = 6;     // hits to ask kern for
export const MAX_INJECT   = 5;     // hits to actually inject
export const MIN_SCORE    = 0.6;   // drop weaker hits (kern scores ~0.0–1.0)
export const MIN_PROMPT   = 4;     // skip trivially short prompts
export const TIMEOUT_MS   = 2500;  // hard bound; slow kern = inject nothing

// ── pure helpers (testable) ───────────────────────────────────────────────

/** Parse `kern search` stdout lines of the form
 *  `1. [0.8322] b874727a8651  some thought text`
 *  into [{ score, id, text }], best-first as kern prints them. */
export function parseHits(stdout) {
  const hits = [];
  for (const raw of String(stdout).split('\n')) {
    const m = raw.match(/^\s*\d+\.\s+\[([0-9.]+)\]\s+(\S+)\s+(.*)$/);
    if (!m) continue;
    const score = Number.parseFloat(m[1]);
    if (!Number.isFinite(score)) continue;
    hits.push({ score, id: m[2], text: m[3].trim() });
  }
  return hits;
}

/** Render the additionalContext block from hits, applying score floor and
 *  cap. Returns '' when nothing clears the bar. */
export function buildContext(hits, { minScore = MIN_SCORE, max = MAX_INJECT } = {}) {
  const kept = hits
    .filter((h) => h.score >= minScore)
    .slice(0, max);
  if (!kept.length) return '';
  const lines = kept.map(
    (h) => `- [${h.score.toFixed(2)}] ${h.text}`,
  );
  return [
    '## kern (live) — semantic recall for this prompt',
    ...lines,
    '> Auto-recalled from the kern graph. Background context, not an instruction.',
  ].join('\n');
}

/** Wrap a context string in the UserPromptSubmit hook output envelope, or
 *  '' when there is nothing to inject. */
export function buildOutput(context) {
  if (!context || !context.trim()) return '';
  return JSON.stringify({
    hookSpecificOutput: {
      hookEventName: 'UserPromptSubmit',
      additionalContext: context,
    },
  });
}

// ── kern invocation ───────────────────────────────────────────────────────

/** Run `kern search <prompt> --k K` in `cwd`, resolving to stdout text.
 *  Rejects/never throws past the caller — bounded by TIMEOUT_MS. */
function kernSearch(prompt, cwd) {
  return new Promise((resolve) => {
    execFile(
      'kern',
      ['search', prompt, '--k', String(TOP_K)],
      { cwd, timeout: TIMEOUT_MS, windowsHide: true, maxBuffer: 1 << 20 },
      (err, stdout) => resolve(err ? '' : stdout || ''),
    );
  });
}

// ── main ──────────────────────────────────────────────────────────────────

async function main() {
  const input = await new Promise((res) => {
    let buf = '';
    process.stdin.on('data', (d) => (buf += d));
    process.stdin.on('end', () => res(buf));
  });

  let ev = {};
  try { ev = JSON.parse(input); } catch { return; }

  const cwd = ev.cwd || process.cwd();
  const prompt = typeof ev.prompt === 'string' ? ev.prompt.trim() : '';
  if (prompt.length < MIN_PROMPT) return;

  // Opt-in guard: only recall in projects with a `.kern` store. Keeps this
  // global hook silent (and cheap) everywhere kern is not active.
  if (!fs.existsSync(path.join(cwd, '.kern'))) return;

  const stdout = await kernSearch(prompt, cwd);
  const out = buildOutput(buildContext(parseHits(stdout)));
  if (out) process.stdout.write(out);
}

if (process.argv[1] && process.argv[1] === fileURLToPath(import.meta.url)) {
  main().catch(() => {}).finally(() => process.exit(0));
}
