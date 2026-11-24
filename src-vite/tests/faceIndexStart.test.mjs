import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { parse } from 'vue/compiler-sfc';
import * as Vue from 'vue';

test('both Process buttons start an incremental scan when clicked', async () => {
  const file = await readFile(new URL('../src/components/Person.vue', import.meta.url), 'utf8');
  const { descriptor } = parse(file);
  const buttons = descriptor.template.content.match(/<button\b[^>]*@click="clickIndexFaces[^"]*"[^>]*>/g);
  assert.equal(buttons.length, 2);
  for (const button of buttons) {
    let rescan;
    const render = Vue.compile(button + '</button>');
    const vnode = render({ clickIndexFaces(value = false) { rescan = value; } }, []);
    vnode.props.onClick(new Event('click'));
    assert.equal(rescan, false);
  }
});
