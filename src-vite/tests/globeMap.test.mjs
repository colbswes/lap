import test from 'node:test'
import assert from 'node:assert/strict'
import { buildGlobeStyle, expandTileUrls, globeCellsBounds, gpsCellKey } from '../src/common/globeMap.js'

test('gpsCellKey rounds half away from zero like SQLite ROUND(x, 2)', () => {
  assert.equal(gpsCellKey(0.125, -0.125), '13:-13')
  assert.equal(gpsCellKey(48.8566, 2.3522), '4886:235')
  assert.notEqual(gpsCellKey(48.854, 2.35), gpsCellKey(48.856, 2.35))
})

test('globeCellsBounds covers every cell the points fall in', () => {
  const points = [{ lat: 10.011, lon: 20.019 }, { lat: 10.03, lon: 19.99 }]
  const bounds = globeCellsBounds(points)
  for (const lat of [10.005, 10.0349]) assert.ok(lat >= bounds.minLat && lat <= bounds.maxLat)
  for (const lon of [19.985, 20.0249]) assert.ok(lon >= bounds.minLon && lon <= bounds.maxLon)
})

test('expandTileUrls turns {s} subdomains into explicit tile URLs', () => {
  assert.deepEqual(expandTileUrls('https://t{s}.x/{z}', '01'), ['https://t0.x/{z}', 'https://t1.x/{z}'])
  assert.deepEqual(expandTileUrls('https://x/{z}/{x}/{y}.png'), ['https://x/{z}/{x}/{y}.png'])
})

test('buildGlobeStyle uses the globe projection with one raster layer per theme layer', () => {
  const style = buildGlobeStyle({ maxZoom: 18, layers: [{ url: 'https://a/{z}', attribution: 'A' }, { url: 'https://b/{z}', attribution: 'B' }] })
  assert.equal(style.projection.type, 'globe')
  assert.deepEqual(style.layers.map(layer => layer.type), ['raster', 'raster'])
})
