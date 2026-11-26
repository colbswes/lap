import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import ts from 'typescript';

const source = fs.readFileSync(new URL('../src/components/Video.vue', import.meta.url), 'utf8');
const start = source.indexOf('  const loadDirect = async');
const code = ts.transpile(source.slice(start, source.indexOf('  // Matroska files', start)), { target: ts.ScriptTarget.ES2022 });

const utils = fs.readFileSync(new URL('../src/common/utils.ts', import.meta.url), 'utf8');
const mediaUrlStart = utils.indexOf('let originalMediaBaseUrl');
const mediaUrlCode = ts.transpile(
  utils.slice(mediaUrlStart, utils.indexOf('// get country name', mediaUrlStart)).replaceAll('export ', ''),
  { target: ts.ScriptTarget.ES2022 },
);
const mediaBase = 'http://127.0.0.1:54321/media/token/';
const media = new Function('invoke', 'useUIStore', `
  ${mediaUrlCode}
  return { initializeOriginalMedia, getAssetSrc };
`)(
  async () => mediaBase,
  () => ({ getFileVersion: () => 2 }),
);
await media.initializeOriginalMedia();

function setup(filePath) {
  const calls = [], listeners = {}, timers = [];
  const player = {
    reset() { calls.push('reset'); },
    src(value) { calls.push(value); },
    one(name, fn) { listeners[name] = fn; },
    off(name) { delete listeners[name]; },
    load() { calls.push('load'); },
    videoWidth: () => 1920,
    videoHeight: () => 1080,
  };
  const create = new Function('filePath', 'props', 'getAssetSrc', 'loadPlayer', 'resetCompatibilityProcessing', 'loadPrepared', 'handleSuccessfulLoad', 'loadAttemptCleanups', 'setTimeout', 'clearTimeout', 'DIRECT_PLAYBACK_TIMEOUT_MS', 'isApplePhotosPath', 'APPLE_PHOTOS_PLAYBACK_TIMEOUT_MS', `
    let currentLoadId = 1, currentLoadingId = 1, nextUpIndex = 0;
    ${code}
    return { load: loadDirect };
  `);
  const controls = create(filePath, { fileVersion: 3 }, media.getAssetSrc, player, () => {}, mode => calls.push(['prepare', mode]), () => calls.push('success'), [], fn => { timers.push(fn); return timers.length; }, () => {}, 8000, () => false, 300000);
  return { ...controls, calls, listeners, timers };
}

test('direct playback uses the local media transport and retains fallback', async () => {
  for (const path of ['/nas/clip.mp4', '/Volumes/clip.mov', 'C:\\Videos\\clip.webm']) {
    const t = setup(path);
    await t.load(null);
    assert.deepEqual(t.calls[0], { src: `${mediaBase}${encodeURIComponent(path)}?v=3&u=2` });
    assert.ok(!t.calls.some(c => c?.[0] === 'prepare'));
    t.listeners.error();
    t.timers[0]();
    assert.equal(t.calls.filter(c => c?.[0] === 'prepare').length, 1);
  }
});

test('original image URLs preserve special characters and cache versions', () => {
  const path = '/Photos/图 + #?.png';
  const url = new URL(media.getAssetSrc(path, 17));
  assert.equal(decodeURIComponent(url.pathname.split('/').at(-1)), path);
  assert.equal(url.searchParams.get('v'), '17');
  assert.equal(url.searchParams.get('u'), '2');
});

test('direct timeout retains compatibility fallback while a loaded frame settles the attempt', async () => {
  const stalled = setup('/clip.mp4');
  await stalled.load('video/mp4');
  stalled.timers[0]();
  assert.deepEqual(stalled.calls.at(-1), ['prepare', 'compatible']);

  const loaded = setup('/clip.mp4');
  await loaded.load('video/mp4');
  loaded.listeners.loadeddata();
  loaded.timers[0]();
  assert.equal(loaded.calls.at(-1), 'success');
  assert.ok(!loaded.calls.some(c => c?.[0] === 'prepare'));
});

function videoLoadFixture(cloudOnly = false) {
  const start = source.indexOf('function cancelVideoLoad()');
  const body = ts.transpile(source.slice(start, source.indexOf('function getFallbackErrorMessage()', start)), {
    target: ts.ScriptTarget.ES2022,
  });
  const calls = [];
  const mockPlayer = index => {
    const listeners = new Map();
    return {
      listeners,
      isDisposed: () => false, pause: () => calls.push([index, 'pause']),
      reset: () => calls.push([index, 'reset']), muted: value => calls.push([index, 'mute', value]),
      volume() {}, src: value => calls.push([index, 'source', value.src]), load() {},
      one: (name, fn) => listeners.set(name, fn),
      off: (name, fn) => { if (listeners.get(name) === fn) listeners.delete(name); },
      videoWidth: () => 1920, videoHeight: () => 1080, play: async () => { calls.push([index, 'play']); },
    };
  };
  const ref = value => ({ value });
  const timers = new Map();
  let timerId = 0;
  const deps = {
    players: ref([mockPlayer(0), mockPlayer(1)]), activeVideo: ref(0), frameReady: ref(false),
    hasPlaybackInteraction: ref(false), isSeeking: ref(false), isPlaying: ref(false), isReplaying: ref(false),
    hasError: ref(false), errorMessage: ref(''), failedPoster: ref(''), isLoading: ref(false), showSpinner: ref(false),
    noTransition: ref(false), isFit: ref(true), rotate: ref(0), viewportOffset: ref({}),
    config: { video: { muted: false, volume: 1 }, settings: { autoPlayVideo: true } },
    props: { filePath: '/start.mp4', isActive: true, fileVersion: 1 },
    clearPlaybackFeedback() {}, clearPlaybackDeadlineTimer() {}, getPlayerId: index => String(index),
    resetCompatibilityProcessing() {}, startCompatibilityProcessing() {},
    resetLoadingUi() {}, shouldBypassDirectPlayback: () => false, isWebViewVideoPlaybackDisabled: () => false,
    applyViewportState() {}, updateTransform() {}, getDirectPlaybackType: () => null,
    getAssetSrc: media.getAssetSrc, $t: key => key,
    recreatePlayer: () => { calls.push(['recreate']); },
    isCloudOnlyFile: async path => { calls.push(['cloud', path]); return cloudOnly; },
    prepareVideo: async path => { calls.push(['prepare', path]); return { url: media.getAssetSrc(path) }; },
    cancelVideoPrepare: async (...args) => calls.push(['cancel', ...args]),
    PLAYBACK_DEADLINE_MS: 10000, LOADING_OVERLAY_DELAY_MS: 500, DIRECT_PLAYBACK_TIMEOUT_MS: 5000,
    isApplePhotosPath: () => false, APPLE_PHOTOS_PLAYBACK_TIMEOUT_MS: 300000,
    setTimeout: (fn, delay) => { timers.set(++timerId, { fn, delay }); return timerId; },
    clearTimeout: id => timers.delete(id),
  };
  const schedule = new Function(...Object.keys(deps), `
    let currentLoadingId = 0, nextBackendRequestId = 0, playbackDeadlineTimer, loadingOverlayTimer, videoLoadTimer;
    const activeBackendRequestIds = [null, null], loadAttemptCleanups = [null, null];
    ${body}
    return scheduleVideoLoad;
  `)(...Object.values(deps));
  return { schedule, calls, timers, ...deps };
}

test('rapid video switches cancel pending listeners and reuse the same player when the final frame arrives', () => {
  const f = videoLoadFixture();
  const [, next] = f.players.value;
  f.schedule('/first.mp4');
  const staleFrame = next.listeners.get('loadeddata');
  f.schedule('/second.mp4', 70);
  assert.equal(f.frameReady.value, false);
  assert.equal(next.listeners.size, 0);
  assert.ok(f.calls.some(call => call[0] === 0 && call[1] === 'mute' && call[2] === true));
  f.schedule('/third.mp4', 70);
  const scrubTimers = [...f.timers.values()].filter(timer => timer.delay === 70);
  assert.equal(scrubTimers.length, 1);
  scrubTimers[0].fn();
  assert.deepEqual(f.calls.filter(call => call[1] === 'source').map(call => decodeURIComponent(call[2])), [
    `${mediaBase}/first.mp4?v=1&u=2`, `${mediaBase}/third.mp4?v=1&u=2`,
  ]);
  staleFrame();
  assert.equal(f.frameReady.value, false);
  next.listeners.get('loadeddata')();
  assert.equal(f.frameReady.value, true);
  assert.equal(f.activeVideo.value, 1);
  assert.ok(f.calls.some(call => call[0] === 1 && call[1] === 'play'));
  assert.ok(!f.calls.some(call => ['cloud', 'prepare', 'recreate'].includes(call[0])));
});

test('a Live Photo activated during loading plays when its first frame arrives', () => {
  const f = videoLoadFixture();
  f.config.settings.autoPlayVideo = false;
  f.props.isActive = false;
  f.props.playOnActivate = true;
  f.schedule('/live.mov');
  assert.ok(!f.calls.some(call => call[1] === 'play'));
  f.props.isActive = true;
  f.players.value[1].listeners.get('loadeddata')();
  assert.ok(f.calls.some(call => call[0] === 1 && call[1] === 'play'));
});

test('failed direct playback checks for cloud-only storage before preparing the video', async () => {
  const f = videoLoadFixture(true);
  f.schedule('/cloud.mp4');
  assert.ok(!f.calls.some(call => call[0] === 'cloud'));
  f.players.value[1].listeners.get('error')();
  await Promise.resolve();
  assert.deepEqual(f.calls.filter(call => call[0] === 'cloud'), [['cloud', '/cloud.mp4']]);
  assert.ok(!f.calls.some(call => call[0] === 'prepare'));
  assert.equal(f.hasError.value, true);
  assert.equal(f.errorMessage.value, 'offline.requires_original');
});
