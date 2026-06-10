import { test } from 'node:test';
import assert from 'node:assert/strict';
import { buildOutput } from '../kern-recall.mjs';

// ── buildOutput ───────────────────────────────────────────────────────────

test('empty string returns empty string', () => {
  assert.equal(buildOutput(''), '');
});

test('whitespace-only returns empty string', () => {
  assert.equal(buildOutput('   '), '');
  assert.equal(buildOutput('\n\t\n'), '');
});

test('null/undefined returns empty string', () => {
  assert.equal(buildOutput(null), '');
  assert.equal(buildOutput(undefined), '');
});

test('valid digest returns SessionStart JSON envelope', () => {
  const out = JSON.parse(buildOutput('# kern memory\n- some fact'));
  assert.equal(out.hookSpecificOutput.hookEventName, 'SessionStart');
  assert.equal(out.hookSpecificOutput.additionalContext, '# kern memory\n- some fact');
});

test('digest passed through verbatim', () => {
  const digest = '# kern memory\n\nAnchors: foo, bar\n\n## What I know\n\n- detail one\n';
  const out = JSON.parse(buildOutput(digest));
  assert.equal(out.hookSpecificOutput.additionalContext, digest);
});
