import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";

const source = fs.readFileSync(
  new URL("../src/common/hiddenAlbums.ts", import.meta.url),
  "utf8",
);
const compiled = ts.transpileModule(source, {
  compilerOptions: {
    module: ts.ModuleKind.ESNext,
    target: ts.ScriptTarget.ES2022,
  },
}).outputText;

const mod = await import(
  `data:text/javascript;base64,${Buffer.from(compiled).toString("base64")}`
);

const {
  normalizeHiddenIds,
  normalizeHiddenStringIds,
  isHiddenId,
  isHiddenStringId,
  withToggledHiddenId,
  withToggledHiddenStringId,
  withoutHiddenId,
  withHiddenIds,
  withoutHiddenIds,
  withoutHiddenStringId,
} = mod;

test("normalizes, deduplicates, and sorts positive numeric ids", () => {
  assert.deepEqual(normalizeHiddenIds([5, 2, 5, 0, -3, "bad", 1.5]), [2, 5]);
  assert.deepEqual(normalizeHiddenIds(null), []);
  assert.deepEqual(normalizeHiddenIds(undefined), []);
});

test("toggles, checks, and removes numeric hidden ids", () => {
  let hidden = [];
  hidden = withToggledHiddenId(hidden, 10);
  hidden = withToggledHiddenId(hidden, 4);
  assert.deepEqual(hidden, [4, 10]);
  assert.equal(isHiddenId(hidden, 10), true);
  assert.equal(isHiddenId(hidden, 20), false);

  hidden = withToggledHiddenId(hidden, 10);
  assert.deepEqual(hidden, [4]);
  assert.equal(isHiddenId(hidden, 10), false);

  hidden = withoutHiddenId(hidden, 4);
  assert.deepEqual(hidden, []);

  hidden = withHiddenIds([2, 8], [8, 3, -1, 0, 5]);
  assert.deepEqual(hidden, [2, 3, 5, 8]);
  hidden = withoutHiddenIds(hidden, [3, 8, 99]);
  assert.deepEqual(hidden, [2, 5]);
});

test("normalizes, toggles, checks, and removes smart collection string ids", () => {
  assert.deepEqual(normalizeHiddenStringIds([" b ", "", "a", "b", null]), ["a", "b"]);
  let hidden = [];
  hidden = withToggledHiddenStringId(hidden, "smart-2");
  hidden = withToggledHiddenStringId(hidden, "smart-1");
  assert.deepEqual(hidden, ["smart-1", "smart-2"]);
  assert.equal(isHiddenStringId(hidden, "smart-1"), true);
  assert.equal(isHiddenStringId(hidden, "missing"), false);

  hidden = withToggledHiddenStringId(hidden, "smart-1");
  assert.deepEqual(hidden, ["smart-2"]);
  hidden = withoutHiddenStringId(hidden, "smart-2");
  assert.deepEqual(hidden, []);
});
