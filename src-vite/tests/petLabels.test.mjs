import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { parse } from 'vue/compiler-sfc';
import * as Vue from 'vue';

const en = JSON.parse(await readFile(new URL('../src/locales/en.json', import.meta.url), 'utf8'));

function lookupKey(obj, key) {
  return key.split('.').reduce((acc, part) => acc?.[part], obj);
}

function makeT() {
  return (key, params = {}) => {
    const template = lookupKey(en, key);
    assert.equal(typeof template, 'string', `Missing en.json key: ${key}`);
    return template.replace(/\{(\w+)\}/g, (_, k) => String(params[k] ?? ''));
  };
}

test('en.json defines pet-specific variants for people/person UI copy', () => {
  const petKeys = [
    'tooltip.not_found.pet',
    'menu.person.kind_pets',
    'menu.person.people_and_pets',
    'menu.person.unnamed_pet',
    'menu.person.search_pets',
    'menu.person.hide_pet',
    'menu.person.show_pet',
    'menu.person.show_all_pets',
    'menu.person.merge_failed_pet',
    'menu.person.edit_pet',
    'menu.person.assign_dialog_hint_pet',
    'menu.person.assign_whole_group_pet',
    'msgbox.delete_person.title_pet',
    'msgbox.delete_person.title_multiple',
    'msgbox.delete_person.title_multiple_pet',
    'msgbox.merge_person.title_pet',
    'msgbox.merge_person.content_pet',
  ];
  for (const key of petKeys) {
    const val = lookupKey(en, key);
    assert.equal(typeof val, 'string', `Missing key: ${key}`);
    assert.ok(val.length > 0, `Empty key: ${key}`);
    if (key.endsWith('_pet') || key === 'tooltip.not_found.pet' || key === 'menu.person.search_pets') {
      assert.doesNotMatch(val, /\b(people|person)\b/i, `Pet key "${key}" should not say people/person: ${val}`);
    }
  }
});

test('Person.vue header, search placeholder, empty states, and dialogs use pet copy when Pets is selected', async () => {
  const file = await readFile(new URL('../src/components/Person.vue', import.meta.url), 'utf8');
  const { descriptor } = parse(file);
  const tpl = descriptor.template.content;
  const $t = makeT();

  const headerMatch = tpl.match(/<span class="sidebar-panel-header-title[\s\S]*?<\/span>/);
  assert.ok(headerMatch, 'Expected header title span');
  const renderHeader = Vue.compile(headerMatch[0]);
  const personHeader = renderHeader({ isPet: false, titlebar: 'People', allPersonCount: 55, $t }, []);
  const petHeader = renderHeader({ isPet: true, titlebar: 'People', allPersonCount: 55, $t }, []);
  const flattenText = (vnode) => {
    if (vnode == null) return '';
    if (typeof vnode === 'string') return vnode;
    if (Array.isArray(vnode)) return vnode.map(flattenText).join('');
    return flattenText(vnode.children);
  };
  assert.equal(flattenText(personHeader).trim(), 'People (55)');
  assert.equal(flattenText(petHeader).trim(), 'Pets (55)');

  const inputMatch = tpl.match(/<input\b[^>]*v-model="personSearch"[^>]*>/);
  assert.ok(inputMatch, 'Expected search input');
  const renderInput = Vue.compile(inputMatch[0]);
  const personInput = renderInput({ isPet: false, personSearch: '', personSearchDisabled: false, isPersonSearchFocused: false, $t }, []);
  const petInput = renderInput({ isPet: true, personSearch: '', personSearchDisabled: false, isPersonSearchFocused: false, $t }, []);
  assert.equal(personInput.props.placeholder, 'Search people');
  assert.equal(petInput.props.placeholder, 'Search pets');

  // Empty states & dialogs must switch on isPet
  assert.doesNotMatch(tpl, /\$t\('tooltip\.not_found\.person'\)/);
  assert.doesNotMatch(tpl, /\$t\('msgbox\.delete_person\.title'\)/);
  assert.doesNotMatch(tpl, /\$t\('msgbox\.merge_person\.title'\)/);
  assert.doesNotMatch(tpl, /\$t\('msgbox\.merge_person\.content'/);
});
