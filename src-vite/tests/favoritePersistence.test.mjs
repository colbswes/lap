import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from 'typescript';

const apiSource = fs.readFileSync(new URL('../src/common/api.js', import.meta.url), 'utf8');

const syncSource = fs.readFileSync(new URL('../src/common/metadataSync.ts', import.meta.url), 'utf8');
const syncJs = ts.transpile(
  syncSource.replace(/export /g, ''),
  { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
);
const {
  createMetadataSyncTracker,
  getMatchingPaneFileInfo,
} = new Function(`${syncJs}; return { createMetadataSyncTracker, getMatchingPaneFileInfo };`)();

test('favoriteSync preserves a favorite pressed before getFileInfo resolves and ignores stale loads', async () => {
  const sync = createMetadataSyncTracker('is_favorite');
  let activeFileId = 100;
  let leftFileInfo = { id: 100, is_favorite: false, rotate: 0 };
  const synced = [];
  const toasts = [];
  const pendingGets = new Map();

  const getFileInfo = (id) => new Promise((resolve) => {
    pendingGets.set(id, resolve);
  });

  async function loadLeft(targetId) {
    const req = sync.beginFileInfoLoad('left');
    if (leftFileInfo && Number(leftFileInfo.id) !== targetId) {
      leftFileInfo = null;
    }
    const loaded = await getFileInfo(targetId);
    if (!sync.isCurrentFileInfoLoad('left', req, activeFileId, targetId)) {
      return;
    }
    leftFileInfo = sync.applyPendingMetadata(loaded, req);
  }

  async function setCurrentFavorite(value, persistFn) {
    const currentId = activeFileId;
    const matching = getMatchingPaneFileInfo(leftFileInfo, currentId);
    const knownConfirmed = matching ? Number(matching.is_favorite ?? 0) : null;
    await sync.enqueueMetadataUpdate({
      fileId: currentId,
      value,
      knownConfirmed,
      pane: 'left',
      advance: false,
      applyOptimistic: (id, next) => {
        const curr = getMatchingPaneFileInfo(leftFileInfo, id);
        if (curr) curr.is_favorite = Boolean(next);
      },
      persist: persistFn,
      onCommit: (id, next, settled) => {
        if (settled) {
          const curr = getMatchingPaneFileInfo(leftFileInfo, id);
          if (curr) curr.is_favorite = Boolean(next);
        }
        synced.push([id, next]);
      },
      onError: (id, rollback) => {
        const curr = getMatchingPaneFileInfo(leftFileInfo, id);
        if (curr && rollback !== null) curr.is_favorite = Boolean(rollback);
        toasts.push('error');
      },
    });
  }

  // Navigate to 100 (slow), then immediately to 101
  const load100 = loadLeft(100);
  activeFileId = 101;
  const load101 = loadLeft(101);

  // User favorites 101 BEFORE getFileInfo(101) resolves, and persistence completes first
  await setCurrentFavorite(1, async () => 1);
  assert.deepEqual(synced, [[101, 1]]);

  // Delayed getFileInfo(100) resolves, then delayed getFileInfo(101) resolves with stale DB snapshot
  pendingGets.get(100)({ id: 100, is_favorite: false });
  await load100;
  assert.equal(leftFileInfo, null, 'stale file 100 load must not populate active pane');

  pendingGets.get(101)({ id: 101, is_favorite: false });
  await load101;
  assert.equal(leftFileInfo.id, 101);
  assert.equal(leftFileInfo.is_favorite, true, 'committed favorite must survive delayed stale getFileInfo');
  assert.equal(toasts.length, 0);
});

test('consecutive updates serialize per file and roll back to last confirmed favorite when a later unfavorite fails', async () => {
  const sync = createMetadataSyncTracker('is_favorite');
  const fileId = 200;
  const info = { id: fileId, is_favorite: 0 };
  const synced = [];
  const toasts = [];
  const calls = [];
  const gates = [];

  function trigger(value) {
    const matching = getMatchingPaneFileInfo(info, fileId);
    const knownConfirmed = matching ? Number(matching.is_favorite ?? 0) : null;
    return sync.enqueueMetadataUpdate({
      fileId,
      value,
      knownConfirmed,
      pane: 'left',
      advance: false,
      applyOptimistic: (_id, next) => {
        info.is_favorite = next;
      },
      persist: (_id, next) => {
        calls.push(next);
        return new Promise((resolve) => gates.push(resolve));
      },
      onCommit: (id, next, settled) => {
        if (settled) info.is_favorite = next;
        synced.push([id, next]);
      },
      onError: (_id, rollback) => {
        if (rollback !== null) info.is_favorite = rollback;
        toasts.push(rollback);
      },
    });
  }

  // Start unfavorited (0): favorite (1), then unfavorite (0) while the first write is still in flight
  const first = trigger(1);
  const second = trigger(0);

  assert.equal(info.is_favorite, 0, 'UI reflects latest user intent immediately');
  assert.deepEqual(calls, [1], 'second write must wait until the first finishes (serialized per file)');

  gates[0](1);
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(synced, [[fileId, 1]]);
  assert.deepEqual(calls, [1, 0]);
  assert.equal(info.is_favorite, 0, 'UI still reflects pending unfavorite while it is in flight');

  // The unfavorite fails: UI must roll back to the confirmed favorite, not the original state
  gates[1](null);
  await Promise.all([first, second]);

  assert.equal(info.is_favorite, 1, 'rollback restores last confirmed favorite state');
  assert.deepEqual(toasts, [1]);
});

test('failed earlier write does not flash error or revert when a newer queued intent succeeds', async () => {
  const sync = createMetadataSyncTracker('is_favorite');
  const fileId = 201;
  const info = { id: fileId, is_favorite: 0 };
  const toasts = [];
  const calls = [];
  const gates = [];

  function trigger(value) {
    const matching = getMatchingPaneFileInfo(info, fileId);
    const knownConfirmed = matching ? Number(matching.is_favorite ?? 0) : null;
    return sync.enqueueMetadataUpdate({
      fileId,
      value,
      knownConfirmed,
      pane: 'left',
      advance: false,
      applyOptimistic: (_id, next) => {
        info.is_favorite = next;
      },
      persist: (_id, next) => {
        calls.push(next);
        return new Promise((resolve) => gates.push(resolve));
      },
      onCommit: (_id, next, settled) => {
        if (settled) info.is_favorite = next;
      },
      onError: (_id, rollback) => {
        if (rollback !== null) info.is_favorite = rollback;
        toasts.push(rollback);
      },
    });
  }

  const first = trigger(1);
  const second = trigger(0);
  assert.deepEqual(calls, [1]);

  // First write fails while the newer intent is queued
  gates[0](null);
  await Promise.resolve();
  await Promise.resolve();
  assert.deepEqual(calls, [1, 0]);
  assert.equal(toasts.length, 0);
  assert.equal(info.is_favorite, 0);

  // Second write succeeds
  gates[1](1);
  await Promise.all([first, second]);
  assert.equal(info.is_favorite, 0);
  assert.equal(toasts.length, 0);
});

test('same-value updates use the latest auto-advance intent and pane', async () => {
  for (const [firstAdvance, latestAdvance] of [[false, true], [true, false]]) {
    const sync = createMetadataSyncTracker('is_favorite');
    const commits = [];
    let finish;
    let writes = 0;
    const options = {
      fileId: 300, value: 1, knownConfirmed: 0,
      pane: 'left', advance: firstAdvance,
      applyOptimistic: () => {},
      persist: () => { writes++; return new Promise(resolve => { finish = resolve; }); },
      onCommit: (_id, _value, settled, advanceInfo) => commits.push({ settled, advanceInfo }),
      onError: () => assert.fail('write should succeed'),
    };
    const first = sync.enqueueMetadataUpdate(options);
    const latest = sync.enqueueMetadataUpdate({ ...options, pane: 'right', advance: latestAdvance });
    assert.equal(writes, 1);
    finish(1);
    await Promise.all([first, latest]);
    assert.equal(writes, 1);
    assert.deepEqual(commits, [{ settled: true, advanceInfo: { pane: 'right', advance: latestAdvance } }]);
  }
});

test('is_favorite: serial writes, failed rollback and stale read protection', async () => {
  const field = 'is_favorite';
  const sync = createMetadataSyncTracker(field);
  const load = sync.beginFileInfoLoad('left');
  const info = { id: 500, [field]: false };
  const gates = [];
  const calls = [];
  const options = {
    fileId: 500, knownConfirmed: 0, pane: 'left', advance: false,
    applyOptimistic: (_id, value) => { info[field] = value; },
    persist: (_id, value) => { calls.push(value); return new Promise(resolve => gates.push(resolve)); },
    onCommit: () => {},
    onError: (_id, value) => { info[field] = value; },
  };
  const first = sync.enqueueMetadataUpdate({ ...options, value: 1 });
  const second = sync.enqueueMetadataUpdate({ ...options, value: 0 });
  assert.deepEqual(calls, [1]);
  gates[0](1);
  await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
  assert.deepEqual(calls, [1, 0]);
  gates[1](null);
  await Promise.all([first, second]);
  assert.equal(info[field], 1);
  const loaded = sync.applyPendingMetadata({ id: 500, [field]: 0 }, load);
  assert.equal(loaded[field], true);
});

test('rejected persistence rolls back and allows a later retry', async () => {
  const sync = createMetadataSyncTracker('is_favorite');
  const errors = [];
  const options = { fileId: 600, value: 1, knownConfirmed: 0, pane: 'left', advance: false,
    applyOptimistic: () => {}, persist: async () => { throw Error('write failed'); },
    onCommit: () => {}, onError: (_id, value) => errors.push(value) };
  await sync.enqueueMetadataUpdate(options);
  assert.deepEqual(errors, [0]);
  let committed;
  await sync.enqueueMetadataUpdate({ ...options, persist: async () => 1, onCommit: (_id, value) => { committed = value; } });
  assert.equal(committed, 1);
});

test('setFileFavorite rejects invalid IDs and zero-row writes', async () => {
  const name = 'setFileFavorite';
  const start = apiSource.indexOf(`export async function ${name}`);
  const end = apiSource.indexOf('\n}\n', start) + 3;
  const snippet = apiSource.slice(start, end).replace('export async function ', 'async function ');
  const calls = [];
  let result = 0;
  const api = new Function('invoke', `${snippet}; return ${name};`)(async (...args) => { calls.push(args); return result; });
  assert.equal(await api(0, 1), null);
  assert.equal(await api('5', 1), null);
  assert.equal(calls.length, 0);
  assert.equal(await api(5, 1), null);
  result = 1;
  assert.equal(await api(5, 1), 1);
});

test('grid rollback updates the current file object after list replacement', async () => {
  const source = fs.readFileSync(new URL('../src/components/Content.vue', import.meta.url), 'utf8');
  const start = source.indexOf('async function setSelectedMetadata(');
  const end = source.indexOf('\nconst toggleFavorite', start);
  const snippet = ts.transpile(source.slice(start, end), { target: ts.ScriptTarget.ES2022 });
  const fileList = { value: [{ id: 42, is_favorite: false }] };
  const libConfig = { _libraryId: 'library-a' };
  const gates = [];
  const metadataWriters = { is_favorite: { tracker: createMetadataSyncTracker('is_favorite'),
    persist: () => new Promise(resolve => gates.push(resolve)) } };
  const set = new Function('metadataWriters', 'fileList', 'libConfig', 'selectedItemIndex', 'isRealFileItem',
    'currentContentRequestId', 'syncFileMetaToImageViewer', 'tauriEmit', 'advanceAfterMetadataShortcut', 'toast', 't',
    `${snippet}; return setSelectedMetadata;`)(metadataWriters, fileList, libConfig, { value: 0 }, file => file?.id > 0,
      1, () => {}, () => {}, () => {}, { error: () => {} }, key => key);
  const first = set('is_favorite', 1, false);
  const old = fileList.value[0];
  fileList.value = [{ id: 42, is_favorite: false }];
  const second = set('is_favorite', 0, false);
  assert.equal(fileList.value[0].is_favorite, false);
  gates[0](1);
  await Promise.resolve(); await Promise.resolve(); await Promise.resolve();
  gates[1](null);
  await Promise.all([first, second]);
  assert.equal(fileList.value[0].is_favorite, true);
  assert.notEqual(old, fileList.value[0]);
});

test('auto-advance uses the latest list request after refresh', async () => {
  const sync = createMetadataSyncTracker('is_favorite');
  let finish;
  const intents = [];
  const options = { fileId: 700, value: 1, knownConfirmed: 0, pane: 'left', advance: false, requestId: 10,
    applyOptimistic: () => {}, persist: () => new Promise(resolve => { finish = resolve; }),
    onCommit: (_id, _value, _settled, intent) => intents.push(intent), onError: () => assert.fail('unexpected failure') };
  const first = sync.enqueueMetadataUpdate(options);
  const latest = sync.enqueueMetadataUpdate({ ...options, advance: true, requestId: 11 });
  finish(1);
  await Promise.all([first, latest]);
  assert.deepEqual(intents, [{ pane: 'left', advance: true, requestId: 11 }]);
});
