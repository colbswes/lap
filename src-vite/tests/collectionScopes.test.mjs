import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import ts from "typescript";

const source = await readFile(
  new URL("../src/common/collectionScopes.ts", import.meta.url),
  "utf8",
);
const exports = {};
new Function(
  "exports",
  ts.transpile(source, {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.CommonJS,
  }),
)(exports);

const {
  SCOPE_EVERYTHING,
  SCOPE_EVERYDAY,
  SCOPE_FAMILY,
  createDefaultCollectionScopeState,
  normalizeCollectionScopeState,
  getActiveScopeAlbumIds,
  isAlbumInScope,
  isActiveScopeUnassigned,
  withActiveScope,
  withScopeAlbums,
  withToggledAlbumInScope,
  pruneRemovedAlbumFromScopes,
} = exports;

test("defaults create everyday and family scopes with everything active", () => {
  const state = createDefaultCollectionScopeState();
  assert.equal(state.activeId, SCOPE_EVERYTHING);
  assert.deepEqual(
    state.scopes.map((s) => s.id),
    [SCOPE_EVERYDAY, SCOPE_FAMILY],
  );
  assert.equal(getActiveScopeAlbumIds(state), null);
  assert.equal(isActiveScopeUnassigned(state), false);
});

test("normalization deduplicates album ids, fills missing defaults, and rejects unknown active ids", () => {
  const normalized = normalizeCollectionScopeState({
    active_id: "unknown",
    scopes: [
      { id: SCOPE_EVERYDAY, name: "", album_ids: [4, 2, 4, 0, -3, "bad"] },
    ],
  });
  assert.equal(normalized.activeId, SCOPE_EVERYTHING);
  assert.equal(normalized.scopes.length, 2);
  assert.equal(normalized.scopes[0].name, "Everyday");
  assert.deepEqual(normalized.scopes[0].albumIds, [2, 4]);
  assert.equal(normalized.scopes[1].id, SCOPE_FAMILY);
  assert.deepEqual(normalized.scopes[1].albumIds, []);
});

test("toggling, switching, and pruning album ids keeps scope state consistent", () => {
  let state = createDefaultCollectionScopeState();
  state = withToggledAlbumInScope(state, SCOPE_EVERYDAY, 10);
  state = withToggledAlbumInScope(state, SCOPE_FAMILY, 20);
  state = withActiveScope(state, SCOPE_EVERYDAY);

  assert.deepEqual(getActiveScopeAlbumIds(state), [10]);
  assert.equal(isAlbumInScope(state, SCOPE_EVERYDAY, 10), true);
  assert.equal(isAlbumInScope(state, SCOPE_EVERYDAY, 20), false);
  assert.equal(isAlbumInScope(state, SCOPE_EVERYTHING, 20), true);
  assert.equal(isActiveScopeUnassigned(state, [10, 20]), false);

  // Removing album 10 leaves Everyday unassigned
  state = pruneRemovedAlbumFromScopes(state, 10);
  assert.deepEqual(getActiveScopeAlbumIds(state), []);
  assert.equal(isActiveScopeUnassigned(state, [20]), true);

  // Explicit setScopeAlbums replaces list cleanly
  state = withScopeAlbums(state, SCOPE_FAMILY, [30, 20, 30]);
  state = withActiveScope(state, SCOPE_FAMILY);
  assert.deepEqual(getActiveScopeAlbumIds(state), [20, 30]);
});
