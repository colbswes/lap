import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from 'typescript';
const source = fs.readFileSync(new URL('../src/common/nativeJxl.ts', import.meta.url), 'utf8').replace(/^import type .*;$/m, '');
function fixture() {
  const js = ts.transpile(source.replace(/export /g, ''), { target: ts.ScriptTarget.ES2022 });
  return new Function(`${js}; return loadNativeJxl;`)();
}
const image = src => ({ src, naturalWidth: 1500, naturalHeight: 1000 });
test('native success avoids fallback', async () => {
  const load = fixture();
  assert.deepEqual(await load('original', () => assert.fail('unexpected fallback'), async src => image(src)), image('original'));
});
test('failure falls back once and remembers only that versioned file', async () => {
  const load = fixture(); const calls = [];
  const decode = async src => { calls.push(src); if (src.startsWith('original')) throw Error('unsupported'); return image(src); };
  await load('original?v=1', () => 'preview1', decode);
  await load('original?v=1', () => 'preview1', decode);
  await load('original?v=2', () => 'preview2', decode);
  assert.deepEqual(calls, ['original?v=1', 'preview1', 'preview1', 'original?v=2', 'preview2']);
});
test('fallback failure is surfaced without a retry loop', async () => {
  const load = fixture(); let calls = 0;
  await assert.rejects(load('original', () => 'preview', async () => { calls++; throw Error('bad image'); }));
  assert.equal(calls, 2);
});
test('one rejected file does not disable native decoding for another', async () => {
  const load = fixture();
  await load('bad', () => 'preview', async src => { if (src === 'bad') throw Error('bad'); return image(src); });
  assert.equal((await load('good', () => assert.fail('unexpected fallback'), async src => image(src))).src, 'good');
});


test('a decode rejection after load falls back to a decoded preview', async () => {
  class FakeImage {
    naturalWidth = 1500;
    naturalHeight = 1000;
    set src(value) { this.url = value; queueMicrotask(() => this.onload()); }
    decode() { return this.url === 'original' ? Promise.reject(Error('decode failed')) : Promise.resolve(); }
  }
  const js = ts.transpile(source.replace(/export /g, ''), { target: ts.ScriptTarget.ES2022 });
  const { loadNativeJxl, loadBrowserImage } = new Function('Image', `${js}; return { loadNativeJxl, loadBrowserImage };`)(FakeImage);
  assert.deepEqual(await loadNativeJxl('original', () => 'preview', loadBrowserImage), image('preview'));
});


test('cancelling a native decode does not fall back or mark the file as unsupported', async () => {
  const load = fixture();
  const controller = new AbortController();
  await assert.rejects(load('original', () => assert.fail('unexpected fallback'), async () => {
    controller.abort();
    throw controller.signal.reason;
  }, controller.signal), { name: 'AbortError' });
  assert.deepEqual(await load('original', () => assert.fail('unexpected fallback'), async src => image(src)), image('original'));
});

test('late thumbnail load events cannot change the completed image layout', () => {
  const source = fs.readFileSync(new URL('../src/components/Image.vue', import.meta.url), 'utf8');
  const code = source.slice(source.indexOf('function updateThumbnailLayout('), source.indexOf('async function showOfflinePreview('));
  const fixture = new Function('placeholderLoadId', 'currentLoadingId', 'activeImage', 'getCompatibleLayout',
    ts.transpile(code) + '; return updateThumbnailLayout;');
  const update = fixture(-1, { value: 2 }, { value: 0 }, () => assert.fail('completed image was replaced'));
  update(0, { target: { naturalWidth: 150, naturalHeight: 100 } });
});
