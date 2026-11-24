import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import ts from "typescript";

const source = fs
  .readFileSync(new URL("../src/common/collectionActions.ts", import.meta.url), "utf8")
  // Strip runtime-only imports for isolated pure helper unit tests.
  .replace(/^import .*$/gm, "")
  .replace(/export async function saveFilesAsNewCollection[\s\S]*?\n\}\n/m, "");

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
  pickUniqueCollectionName,
  buildSmartDraftFromTags,
  buildSmartDraftFromCollection,
  buildSmartDuplicateDraft,
} = mod;

test("pickUniqueCollectionName deduplicates case-insensitively", () => {
  assert.equal(pickUniqueCollectionName("Trips", ["Family"]), "Trips");
  assert.equal(pickUniqueCollectionName("Trips", ["trips", "Trips 2"]), "Trips 3");
  assert.equal(pickUniqueCollectionName("  ", []), "Collection");
});

test("buildSmartDraftFromTags creates AND/OR tag rules", () => {
  const single = buildSmartDraftFromTags([{ id: 7, name: "Japan" }], false);
  assert.equal(single.name, "Japan");
  assert.equal(single.query.match, "all");
  assert.deepEqual(single.query.rules, [{ field: "tag", operator: "has", value: 7 }]);

  const multiOr = buildSmartDraftFromTags(
    [
      { id: 7, name: "Japan" },
      { id: 9, name: "Food" },
    ],
    true,
  );
  assert.equal(multiOr.name, "Japan / Food");
  assert.equal(multiOr.query.match, "any");
  assert.equal(multiOr.query.rules.length, 2);
});

test("buildSmartDraftFromCollection creates a collection rule draft", () => {
  const draft = buildSmartDraftFromCollection({ id: 12, name: "Best of 2026" });
  assert.equal(draft.name, "Best of 2026");
  assert.equal(draft.query.match, "all");
  assert.deepEqual(draft.query.rules, [{ field: "collection", operator: "is", value: 12 }]);
});

test("buildSmartDuplicateDraft strips id/timestamps and appends copy suffix", () => {
  const dup = buildSmartDuplicateDraft(
    {
      id: "abc",
      name: "Favorites",
      createdAt: 100,
      updatedAt: 200,
      count: 42,
      query: { version: 1, match: "all", rules: [{ field: "favorite", operator: "is", value: true }] },
    },
    "Copy",
  );
  assert.equal(dup.id, undefined);
  assert.equal(dup.createdAt, undefined);
  assert.equal(dup.count, undefined);
  assert.equal(dup.name, "Favorites Copy");
  assert.equal(dup.query.rules.length, 1);
});
