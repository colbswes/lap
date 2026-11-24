import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const source = readFileSync(new URL('../src/common/personSelection.ts', import.meta.url), 'utf8');
const js = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2020 } }).outputText;
const { nextPersonSelection } = await import(`data:text/javascript,${encodeURIComponent(js)}`);

const order = [1, 2, 3, 4, 5];

test('plain click selects only the clicked person', () => {
  assert.deepEqual(nextPersonSelection(order, { ids: [1, 2], anchorId: 1 }, 4), { ids: [4], anchorId: 4 });
});

test('cmd/ctrl click toggles membership and moves the anchor', () => {
  assert.deepEqual(nextPersonSelection(order, { ids: [2], anchorId: 2 }, 4, { metaKey: true }), { ids: [2, 4], anchorId: 4 });
  assert.deepEqual(nextPersonSelection(order, { ids: [2, 4], anchorId: 4 }, 2, { ctrlKey: true }), { ids: [4], anchorId: 2 });
});

test('shift click selects the range from the anchor in either direction', () => {
  assert.deepEqual(nextPersonSelection(order, { ids: [2], anchorId: 2 }, 4, { shiftKey: true }), { ids: [2, 3, 4], anchorId: 2 });
  assert.deepEqual(nextPersonSelection(order, { ids: [2, 3, 4], anchorId: 4 }, 1, { shiftKey: true }), { ids: [1, 2, 3, 4], anchorId: 4 });
  // A second shift click replaces the previous range.
  assert.deepEqual(nextPersonSelection(order, { ids: [2, 3, 4], anchorId: 2 }, 3, { shiftKey: true }), { ids: [2, 3], anchorId: 2 });
});

test('cmd+shift click adds the range to the existing selection', () => {
  assert.deepEqual(nextPersonSelection(order, { ids: [1, 4], anchorId: 4 }, 5, { shiftKey: true, metaKey: true }), { ids: [1, 4, 5], anchorId: 4 });
});

test('shift click without a visible anchor falls back to a plain click', () => {
  assert.deepEqual(nextPersonSelection(order, { ids: [], anchorId: 99 }, 3, { shiftKey: true }), { ids: [3], anchorId: 3 });
});
