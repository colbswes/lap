import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from 'typescript';
import { computed, markRaw, ref } from 'vue';

const menuSource = fs.readFileSync(new URL('../src/common/fileMenu.ts', import.meta.url), 'utf8');
const locale = JSON.parse(fs.readFileSync(new URL('../src/locales/en.json', import.meta.url), 'utf8'));
const availability = fs.readFileSync(new URL('../src/common/availability.ts', import.meta.url), 'utf8');
const requiresOriginalAction = new Function(
  ts.transpile(availability.slice(availability.indexOf('const originalActions')).replace(/export /g, ''))
    + ';return requiresOriginalAction;'
)();
const icons = Object.fromEntries([...new Set(menuSource.match(/\bIcon\w+/g))].map(name => [name, {}]));
const menuCode = ts.transpile(
  menuSource.replace(/^import[\s\S]*?from ['"][^'"]+['"];?\n/gm, '').replace(/export /g, ''),
  { target: ts.ScriptTarget.ES2022 },
);

function makeMenu({ faceEnabled = true } = {}) {
  const deps = {
    computed,
    markRaw,
    ...icons,
    config: {
      externalAppsFor: () => [],
      defaultExternalApp: () => null,
      main: { sidebarIndex: 0 },
      settings: { face: { enabled: faceEnabled } },
    },
    libConfig: { album: { id: 1 }, activePane: 'album' },
    SIDEBAR: { ALBUM: 1 },
    DEFAULT_PLATFORM: 'mac',
    getShortcutLabel: () => '',
    isWin: false,
    isOriginalUnavailable: () => false,
    isOriginalStorageUnavailable: () => false,
    requiresOriginalAction,
    requiresLocalContent: () => false,
    isPreviewOnlyDocument: () => false,
    personViewContext: ref(null),
  };
  return new Function(...Object.keys(deps), `${menuCode};return useFileMenuItems;`)(...Object.values(deps));
}

const findAction = (items, name) => items.find(item => item.action?.actionName === name);

test('single-video context menu enables add-person, find-person, and scan-faces when face recognition is enabled', () => {
  const useFileMenuItems = makeMenu({ faceEnabled: true });
  const called = [];
  const video = ref({ id: 10, name: 'IMG_0226.MOV', file_type: 2 });
  const menu = useFileMenuItems(video, ref(locale), true, key => key, action => called.push(action));

  for (const name of ['add-person', 'find-person', 'scan-faces']) {
    const item = findAction(menu.value, name);
    assert.ok(item, `expected ${name} menu item`);
    assert.equal(Boolean(item.hidden), false, `${name} should be visible`);
    assert.equal(Boolean(item.disabled), false, `${name} should be enabled on videos`);
    item.action();
  }
  assert.deepEqual(called, ['add-person', 'find-person', 'scan-faces']);
});

test('multi-select context menu offers bulk add-person for selected videos and hides it from SelectionPanel moreActions', () => {
  const useFileMenuItems = makeMenu({ faceEnabled: true });
  const called = [];
  const video = ref({ id: 10, name: 'IMG_0226.MOV', file_type: 2 });
  const options = {
    selectMode: ref(true),
    selectionCount: ref(4),
    selectionMediaKind: ref('video'),
    selectionHasUnavailable: ref(false),
    selectionHasCloudOnly: ref(false),
  };
  const menu = useFileMenuItems(video, ref(locale), true, key => key, action => called.push(action), options);

  const addPerson = findAction(menu.value, 'add-person');
  assert.ok(addPerson, 'expected bulk add-person menu item');
  assert.equal(Boolean(addPerson.hidden), false);
  assert.equal(Boolean(addPerson.disabled), false);
  assert.equal(addPerson.hideInSelectionPanel, true);
  addPerson.action();
  assert.deepEqual(called, ['add-person']);

  options.selectionCount.value = 0;
  assert.equal(findAction(menu.value, 'add-person').disabled, true);
});

test('face actions stay hidden in single and selection menus when face recognition is disabled', () => {
  const useFileMenuItems = makeMenu({ faceEnabled: false });
  const video = ref({ id: 10, name: 'IMG_0226.MOV', file_type: 2 });
  for (const selectMode of [false, true]) {
    const menu = useFileMenuItems(video, ref(locale), true, key => key, () => {}, {
      selectMode: ref(selectMode),
      selectionCount: ref(3),
      selectionMediaKind: ref('video'),
    });
    const addPerson = findAction(menu.value, 'add-person');
    assert.ok(addPerson);
    assert.equal(addPerson.hidden, true);
  }
});

test('FileInfo, MediaViewer, and SelectionPanel expose single-video and bulk person/pet tagging', () => {
  const fileInfo = fs.readFileSync(new URL('../src/components/FileInfo.vue', import.meta.url), 'utf8');
  assert.match(fileInfo, /config\.settings\.face\.enabled && \[1, 2, 3\]\.includes\(Number\(fileInfo\?\.file_type\)\)/);

  const viewer = fs.readFileSync(new URL('../src/components/MediaViewer.vue', import.meta.url), 'utf8');
  assert.match(viewer, /!\[1, 2, 3\]\.includes\(Number\(file\?\.file_type\)\)/);
  assert.match(viewer, /data-video-face-toolbar/);

  const selectionPanel = fs.readFileSync(new URL('../src/components/SelectionPanel.vue', import.meta.url), 'utf8');
  assert.match(selectionPanel, /\$emit\('addPersonAll'\)/);
  assert.match(selectionPanel, /menu\.person\.add_person_or_pet/);
});
