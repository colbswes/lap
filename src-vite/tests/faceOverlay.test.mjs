import test from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import ts from 'typescript';
import * as Vue from 'vue';
import { compileTemplate } from '@vue/compiler-sfc';

const source = readFileSync(new URL('../src/components/Image.vue', import.meta.url), 'utf8');

const fnStart = source.indexOf('function getFaceBoxStyle(');
const fnEnd = source.indexOf('\nconst FACE_CORNERS =', fnStart);
assert.notEqual(fnStart, -1);
assert.notEqual(fnEnd, -1);
const helperJs = ts.transpile(source.slice(fnStart, fnEnd), { target: ts.ScriptTarget.ES2022 });

function makeHelper(props, imageSize, activeImage = 0) {
  return new Function('props', 'imageSize', 'activeImage', `${helperJs}; return getFaceBoxStyle;`)(
    props,
    { value: imageSize },
    { value: activeImage },
  );
}

test('face boxes scale to percentages of original image dimensions when preview is downscaled', () => {
  // 24MP iPhone HEIC (4284x5712) rendered via 4096px-capped backend preview (3072x4096).
  const getFaceBoxStyle = makeHelper(
    { imageWidth: 4284, imageHeight: 5712 },
    [{ width: 3072, height: 4096 }, { width: 0, height: 0 }],
    0,
  );
  const style = getFaceBoxStyle({ x: 1458.17, y: 2779.8, width: 385.58, height: 534.62 });
  assert.ok(style.left.endsWith('%'));
  assert.ok(style.top.endsWith('%'));
  assert.ok(style.width.endsWith('%'));
  assert.ok(style.height.endsWith('%'));
  assert.ok(Math.abs(parseFloat(style.left) - (1458.17 / 4284) * 100) < 1e-6);
  assert.ok(Math.abs(parseFloat(style.top) - (2779.8 / 5712) * 100) < 1e-6);
  assert.ok(Math.abs(parseFloat(style.width) - (385.58 / 4284) * 100) < 1e-6);
  assert.ok(Math.abs(parseFloat(style.height) - (534.62 / 5712) * 100) < 1e-6);
});

test('face boxes fall back to active imageSize when original imageWidth/imageHeight are absent', () => {
  const getFaceBoxStyle = makeHelper(
    { imageWidth: 0, imageHeight: 0 },
    [{ width: 2000, height: 1000 }, { width: 0, height: 0 }],
    0,
  );
  assert.deepEqual(getFaceBoxStyle({ x: 500, y: 250, width: 200, height: 250 }), {
    left: '25%',
    top: '25%',
    width: '10%',
    height: '25%',
  });
});

const overlayTemplate = source.slice(
  source.indexOf('    <!-- Faces Overlay -->'),
  source.indexOf('    <AssignFaceDialog'),
);
const compiled = compileTemplate({
  source: overlayTemplate,
  filename: 'Image.vue',
  id: 'face-overlay-test',
  compilerOptions: { mode: 'function', prefixIdentifiers: true, cacheHandlers: false, hoistStatic: false },
});
assert.deepEqual(compiled.errors, []);
const renderOverlay = new Function('Vue', compiled.code)({
  ...Vue,
  withCtx: fn => fn,
  withDirectives: vnode => vnode,
});

function descendants(vnode) {
  if (!vnode || typeof vnode !== 'object') return [];
  return [vnode, ...(Array.isArray(vnode.children) ? vnode.children.flatMap(descendants) : [])];
}

test('faces overlay renders positioned face boxes only on the matching active image buffer', () => {
  const baseCtx = {
    showFaceOverlay: true,
    faces: [{ id: 1, person_id: 2, person_name: 'Colby', bbox: { x: 100, y: 200, width: 50, height: 60 } }],
    isDraggingImage: false,
    isSlideShow: false,
    isSliding: false,
    noTransition: false,
    isResizingContainer: false,
    isWheelZooming: false,
    isDraggingNavBox: false,
    activeImage: 0,
    imageFilePath: ['/old.heic', '/new.heic'],
    filePath: '/new.heic',
    imageSize: [{ width: 3072, height: 4096 }, { width: 3072, height: 4096 }],
    position: [{ x: 0, y: 0 }, { x: 0, y: 0 }],
    scale: [0.25, 0.25],
    imageRotate: [0, 0],
    showAllFaces: true,
    hoveredFaceId: null,
    libConfig: { person: { id: 0 } },
    config: { main: { sidebarIndex: 0 } },
    SIDEBAR: { PERSON: 5 },
    FACE_CORNERS: [],
    t: k => k,
    getFaceBoxStyle: () => ({ left: '10%', top: '20%', width: '5%', height: '6%' }),
  };

  const mismatchFaces = descendants(renderOverlay(baseCtx, [])).filter(n => n.props?.['data-face-id'] === 1);
  assert.equal(mismatchFaces.length, 0);

  const matchedFaces = descendants(renderOverlay({ ...baseCtx, activeImage: 1 }, [])).filter(n => n.props?.['data-face-id'] === 1);
  assert.equal(matchedFaces.length, 1);
  assert.deepEqual(matchedFaces[0].props.style, { left: '10%', top: '20%', width: '5%', height: '6%' });
});
