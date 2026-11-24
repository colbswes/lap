import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';

const read = path => readFileSync(new URL(path, import.meta.url), 'utf8');
const imageSource = read('../src/components/Image.vue');
const transpile = code => ts.transpile(code, { target: ts.ScriptTarget.ES2022 });
const rawFallbackSource = imageSource.slice(imageSource.indexOf('function isEmbeddedRawFallback('), imageSource.indexOf('let prefetchTimer:'));
const cacheSource = transpile(read('../src/common/viewerImages.ts')
  .replace(/^import type .*;$/m, '').replaceAll('export ', ''));
const cacheFixture = (revokeObjectURL = () => {}) => new Function('URL', `
  ${cacheSource}
  return { getImageNeighbors, getDecodedImage, cacheDecodedImage, clearDecodedImages, retainImageSlot, releaseImageSlot, queueImagePrefetch };
`)({ revokeObjectURL });
const image = src => ({ src, naturalWidth: 800, naturalHeight: 600 });
const flush = () => new Promise(resolve => setImmediate(resolve));

test('neighbor priority follows direction and excludes videos and placeholders', () => {
  const { getImageNeighbors } = cacheFixture();
  const files = [1, 3, 1, 3, 1].map((file_type, id) => ({
    id: id + 1, file_path: `/file-${id + 1}`, file_type, modified_at: id + 50,
  }));
  assert.deepEqual(getImageNeighbors(files, 2, 1).map(file => file.id), [4, 5, 2, 1]);
  assert.deepEqual(getImageNeighbors(files, 2, -1).map(file => file.id), [2, 1, 4, 5]);
  files[0].isPlaceholder = true;
  files[3].file_type = 2;
  files[1].media_subtype = 'raw_jpeg_pair';
  files[1].live_photo_video_path = '/paired.jpg';
  assert.deepEqual(getImageNeighbors(files, 2, -1).map(file => file.id), [2, 5]);
  assert.equal(getImageNeighbors(files, 2, -1)[0].live_photo_video_path, '/paired.jpg');
  assert.deepEqual(getImageNeighbors(files, -1, 1), []);
});

test('LRU retains recently used entries and revokes evicted RAW URLs after both slots release them', () => {
  const revoked = [];
  const c = cacheFixture(src => revoked.push(src));
  const raw = { ...image('blob:raw'), raw: { source: 'embedded' } };
  c.cacheDecodedImage('raw', '/raw.dng', raw);
  c.retainImageSlot(raw.src);
  c.retainImageSlot(raw.src);
  for (let i = 0; i < 11; i++) c.cacheDecodedImage(String(i), `/${i}.jpg`, image(String(i)));
  assert.equal(c.getDecodedImage('raw'), raw);
  c.cacheDecodedImage('new', '/new.jpg', image('new'));
  assert.equal(c.getDecodedImage('0'), undefined);
  assert.equal(c.getDecodedImage('raw'), raw);
  c.clearDecodedImages('/raw.dng');
  assert.equal(c.getDecodedImage('raw'), undefined);
  c.releaseImageSlot(raw.src);
  assert.deepEqual(revoked, []);
  c.releaseImageSlot(raw.src);
  assert.deepEqual(revoked, ['blob:raw']);
});

test('prefetch concurrency stays bounded and cancelled queued work never starts', async () => {
  const { queueImagePrefetch } = cacheFixture();
  const started = [];
  const finish = [];
  const controllers = Array.from({ length: 4 }, () => new AbortController());
  const jobs = controllers.map(({ signal }, i) => queueImagePrefetch(signal, () => new Promise(resolve => {
    started.push(i);
    finish[i] = resolve;
  })));
  await flush();
  assert.deepEqual(started, [0, 1]);
  controllers[2].abort();
  finish[1]();
  await flush();
  assert.deepEqual(started, [0, 1, 3]);
  finish[0]();
  finish[3]();
  await Promise.all(jobs);
});

test('preview requests set the viewer for active decodes and a prefetch flag for warming', () => {
  const code = imageSource.slice(imageSource.indexOf('function previewRequestUrl('), imageSource.indexOf('function decodedImageKey('));
  const url = new Function('getPreviewUrl', 'rawViewerId', transpile(code) + '; return previewRequestUrl;')(
    (id, _path, _bust, version) => `preview://localhost/library/${id}?v=${version}`, 'pane-1',
  );
  const heic = { id: 5, file_type: 1, file_path: '/file.heic', modified_at: 42 };
  assert.equal(url(heic, false).searchParams.get('viewerId'), 'pane-1');
  const background = url(heic, true);
  assert.equal(background.searchParams.get('v'), '42');
  assert.equal(background.searchParams.get('prefetch'), 'true');
  assert.equal(background.searchParams.has('viewerId'), false);
});

test('RAW warming discards embedded fallbacks for rendered modes and releases their URLs', async () => {
  const revoked = [];
  const cached = [];
  let options;
  let run;
  const source = imageSource.slice(imageSource.indexOf('let prefetchTimer:'), imageSource.indexOf('function loadPlaceholderResource('));
  const deps = {
    props: { filePath: '/current.dng', prefetchFiles: [{ file_path: '/next.dng', file_type: 3 }] },
    libConfig: { _libraryId: 'library' },
    getRawDisplayOptions: () => options,
    setTimeout: fn => { run = fn; },
    queueImagePrefetch: (_signal, load) => load(),
    decodedImageKey: () => 'key',
    getDecodedImage: () => undefined,
    loadImageResource: async () => ({ ...image('blob:fallback'), raw: { source: 'embedded' } }),
    releaseRawUrl: src => revoked.push(src),
    cacheDecodedImage: (_key, _path, result) => cached.push(result.src),
  };
  const warm = new Function(...Object.keys(deps), transpile(rawFallbackSource + source) + '; return warmImages;')(...Object.values(deps));
  for (options of [
    { mode: 'rendered', autoBright: false, preferPair: false },
    { mode: 'rendered', autoBright: true, preferPair: false },
    { mode: 'embedded', autoBright: false, preferPair: false },
    { mode: 'rendered', autoBright: false, preferPair: true },
  ]) {
    warm();
    await run();
  }
  assert.deepEqual(revoked, ['blob:fallback', 'blob:fallback']);
  assert.deepEqual(cached, ['blob:fallback', 'blob:fallback']);
});

test('foreground loading uses cache hits and displays RAW fallbacks without caching them', async () => {
  const a = imageSource.indexOf('// Watch file changes and the selected RAW preview source.');
  const b = imageSource.indexOf('watch(displayThumbnailSrc', a);
  const code = transpile(rawFallbackSource + imageSource.slice(a, b));
  let select, decode;
  const displayed = [], warmed = [], timers = [], cached = [];
  const ref = value => ({ value });
  const props = { fileType: 1, showThumbnailPlaceholder: true, loadDelay: 70 };
  const deps = {
    props, watch: (_source, callback) => { select = callback; },
    currentLoadingId: ref(0), isLoading: ref(false), loadError: ref(false), cloudLoadError: ref(false),
    offlinePreview: ref(false), maxScale: ref(8), window: {},
    cancelWarmImageScheduling() {}, currentImageFile: path => ({ file_type: props.fileType, file_path: path }),
    requestedRawOptions: ref({}), decodedImageKey: file => file.file_path,
    getDecodedImage: key => key === '/cached.jpg' ? image('cached') : undefined,
    isWin: false, displayThumbnailSrc: ref('thumbnail'),
    showThumbnail: src => displayed.push(src),
    setTimeout: (callback, delay) => { timers.push({ callback, delay }); return timers.length; },
    clearTimeout() {},
    loadImageResource: (file, _options, signal) => new Promise(resolve => { decode = { file, signal, resolve }; }),
    cacheDecodedImage: key => cached.push(key), isZoomFit: ref(true), imageFilePath: ref(['', '']), imageSize: ref([{}, {}]),
    activeImage: ref(0), scale: ref([1, 1]), position: ref([{}, {}]),
    setImageSlot: (_slot, _path, src) => displayed.push(src), onImageReady() {},
    rawRequestPending: ref(false), rawSource: ref(''), rawEmbeddedUnavailable: ref(false), failedRawModes: ref(new Set()),
    getCompatibleLayout: (width, height) => ({ width, height }),
    warmImages: () => warmed.push(true),
  };
  new Function(...Object.keys(deps), `
    let rawUpgradeTimer, rawAbortController, imageLoadController, loadingTimeout, placeholderLoadId, readyLoadId;
    ${code}
  `)(...Object.values(deps));

  await select(['/cached.jpg'], ['/previous.jpg']);
  assert.deepEqual(displayed, ['cached']);
  assert.equal(timers.length, 0);
  assert.equal(decode, undefined);
  const pending = select(['/new.jpg'], ['/cached.jpg']);
  assert.deepEqual(displayed, ['cached', 'thumbnail']);
  assert.equal(decode, undefined);
  timers.find(timer => timer.delay === 70).callback();
  await flush();
  assert.equal(decode.file.file_path, '/new.jpg');
  decode.resolve(image('original'));
  await pending;
  assert.deepEqual(displayed, ['cached', 'thumbnail', 'original']);
  assert.equal(warmed.length, 2);

  props.fileType = 3;
  props.loadDelay = 0;
  deps.requestedRawOptions.value = { mode: 'rendered', autoBright: false, preferPair: false };
  const raw = select(['/new.dng'], ['/new.jpg']);
  decode.resolve({ ...image('blob:fallback'), raw: { source: 'embedded', pair: null } });
  await raw;
  assert.equal(displayed.at(-1), 'blob:fallback');
  assert.deepEqual(cached, ['/new.jpg']);
});

test('standalone viewer accepts pushed media immediately and retains it during a fallback lookup', async () => {
  const source = read('../src/views/ImageViewer.vue');
  const start = source.indexOf('async function loadPaneFileInfo(');
  let current = { id: 1, is_favorite: false };
  let target = 2;
  let load;
  const deps = {
    libConfig: { _libraryId: 'library' },
    favoriteSync: {
      beginFileInfoLoad: () => 1,
      isCurrentFileInfoLoad: (_pane, _request, active, requested) => active === requested,
      applyPendingMetadata: (file, request) => {
        if (file?.id === 2 && request === undefined) file.is_favorite = true;
        return file;
      },
    },
    getMatchingPaneFileInfo: (file, id) => file?.id === id ? file : null,
    getFileInfo: () => new Promise(resolve => { load = resolve; }),
    getFileIdByPane: () => target,
    setFileInfoByPane: (_pane, file) => { current = file; },
  };
  const update = new Function(...Object.keys(deps), `
    ${transpile(source.slice(start, source.indexOf('function setFileInfoByPane(', start)))}
    return loadPaneFileInfo;
  `)(...Object.values(deps));
  const pushed = { id: 2, is_favorite: false };
  const pushedUpdate = update('right', 2, pushed);
  assert.deepEqual(current, { id: 2, is_favorite: true });
  assert.equal(pushed.is_favorite, false);
  assert.equal(load, undefined);
  await pushedUpdate;
  target = 3;
  const pending = update('right', 3);
  assert.equal(current.id, 2);
  load({ id: 3, is_favorite: false });
  await pending;
  assert.equal(current.id, 3);
});

test('viewer navigation loads a grouped placeholder by row and rejects unresolved targets', async () => {
  const source = read('../src/components/Content.vue');
  const start = source.indexOf("case 'request-file-at-index':");
  const end = source.indexOf("case 'update-file-meta':", start);
  const file = { id: 42, file_path: '/target.jpg' };
  const files = [{ id: 41 }, { isPlaceholder: true }, { isPlaceholder: true }];
  const session = { mode: 'normal' };
  const requests = [];
  const replies = [];
  const deps = {
    fileList: { value: files }, imageViewerSession: { value: session }, currentContentRequestId: 1,
    viewerNavigationRequests: new Map(), selectedItemIndex: { value: 0 }, groupedModeActive: { value: true },
    isRealFileItem: item => item && !item.isPlaceholder,
    ensureGroupedFileAtIndex: () => false,
    getGroupedRowIndexForFileIndex: index => index + 5,
    fetchGroupedRowsRange: async (...args) => {
      requests.push(['grouped', ...args]);
      if (args[0] === 6) files[1] = file;
    },
    fetchDataRange: async (...args) => requests.push(['flat', ...args]),
    nextTick: async callback => callback(),
    WebviewWindow: { getByLabel: async () => ({ emit: async (_message, reply) => replies.push(reply) }) },
    getImageNeighbors: () => [],
  };
  const navigate = new Function(...Object.keys(deps), transpile(`
    let suppressImageViewerSync = false;
    return async (event: any) => {
      switch (event.payload.message) {
        ${source.slice(start, end)}
      }
    };
  `))(...Object.values(deps));

  await navigate({ payload: { message: 'request-file-at-index', index: 1, navigationRequest: 1 } });
  assert.deepEqual(requests[0], ['grouped', 6, 7, false, true]);
  assert.equal(replies[0].file, file);
  assert.equal(deps.selectedItemIndex.value, 1);
  await navigate({ payload: { message: 'request-file-at-index', index: 2, navigationRequest: 2 } });
  assert.equal(replies[1].navigationRejected, true);
  assert.equal(replies[1].navigationRequest, 2);
  assert.equal(deps.selectedItemIndex.value, 1);

  deps.groupedModeActive.value = false;
  await navigate({ payload: { message: 'request-file-at-index', index: 2, navigationRequest: 3 } });
  assert.deepEqual(requests.at(-1), ['flat', 0, 5, false, true]);
  assert.equal(replies[2].navigationRejected, true);
});

test('viewer rejection, failed IPC and timeout allow navigation to retry the same target', async () => {
  const source = read('../src/views/ImageViewer.vue');
  const listenerStart = source.indexOf("async (event: any) => {", source.indexOf("listen('update-img'"));
  const listener = source.slice(listenerStart, source.indexOf('\n  });', listenerStart)) + '\n}';
  const request = source.slice(source.indexOf('function clearPendingNavigation('), source.indexOf('function getFileInfoByPane('));
  const next = source.slice(source.indexOf('function clickNext('), source.indexOf('function clickHome('));
  const messages = [];
  const timers = new Map();
  let timerId = 0;
  let failEmission = false;
  const deps = {
    fileCount: { value: 2 }, allPanes: ['left'], isSlideShow: { value: false },
    uiStore: { inputStack: [] }, getFileIndexByPane: () => 0,
    setActivePane() {}, getViewerRef: () => null,
    localeMsg: { value: { tooltip: { image_viewer: {} } } },
    setTimeout: callback => { timers.set(++timerId, callback); return timerId; },
    clearTimeout: id => timers.delete(id),
    emit: async (_name, message) => {
      messages.push(message);
      if (failEmission) throw new Error('IPC failed');
    },
  };
  const { receive, clickNext, requestFileAtIndex } = new Function(...Object.keys(deps), transpile(`
    const pendingNavigation = new Map();
    let navigationRequest = 0;
    const NAVIGATION_TIMEOUT_MS = 5000;
    ${request}
    ${next}
    return { receive: ${listener}, clickNext, requestFileAtIndex };
  `))(...Object.values(deps));

  requestFileAtIndex(-1);
  requestFileAtIndex(2);
  assert.equal(messages.length, 0);
  clickNext();
  const first = messages.at(-1);
  await receive({ payload: { navigationRequest: first.navigationRequest, navigationRejected: true } });
  assert.equal(timers.size, 0);
  clickNext();
  await receive({ payload: { navigationRequest: first.navigationRequest, navigationRejected: true } });
  assert.equal(timers.size, 1, 'a stale rejection cannot clear the newer request');
  timers.values().next().value();
  failEmission = true;
  clickNext();
  await flush();
  assert.equal(timers.size, 0);
  failEmission = false;
  clickNext();
  assert.deepEqual(messages.map(message => message.index), [1, 1, 1, 1]);
});
