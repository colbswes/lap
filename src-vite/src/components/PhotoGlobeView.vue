<template>
  <div
    class="photo-globe-view group/map relative flex-1 overflow-hidden"
    @mouseenter="uiStore.setMapActive(true)"
    @mouseleave="uiStore.setMapActive(false)"
  >
    <div v-if="loading" class="absolute inset-0 z-50 flex items-center justify-center bg-base-200/50">
      <span class="loading loading-spinner loading-md text-primary"></span>
    </div>
    <div ref="mapEl" class="h-full w-full"></div>
    <div class="absolute top-2 left-2 z-500 flex cursor-pointer rounded-box bg-base-100/30 opacity-0 pointer-events-none transition-opacity duration-150 group-hover/map:bg-base-100/70 group-hover/map:opacity-100 group-hover/map:pointer-events-auto">
      <TButton :icon="IconZoomOut" :tooltip="t('map.zoom_out')" :disabled="zoom <= 0" @click="zoomOut" />
      <TButton :icon="IconZoomIn" :tooltip="t('map.zoom_in')" :disabled="zoom >= activeMaxZoom" @click="zoomIn" />
      <TButton :icon="IconMapCenter" :tooltip="t('map.zoom_center')" @click="frameView(true)" />
      <TButton
        :icon="config.settings.mapTheme === 0 ? IconMapDefault : IconMapSatellite"
        :tooltip="t(config.settings.mapTheme === 0 ? 'map.standard' : 'map.satellite')"
        @click="config.settings.mapTheme = config.settings.mapTheme === 0 ? 1 : 0"
      />
      <TButton :icon="IconMapDefault" :tooltip="t('map.flat_map')" @click="config.settings.mapGlobe = false" />
    </div>
    <div v-if="!loading" class="absolute top-2 right-2 z-500 pointer-events-none rounded-box bg-base-100/60 px-2 py-1 text-xs text-base-content/70">
      {{ points.length > 0 ? t('map.photo_count', { count: totalCount.toLocaleString() }) : t('map.no_photos_in_view') }}
    </div>
  </div>
</template>

<script setup>
import { MAP_SINGLE_POINT_FIT_ZOOM } from '@/common/constants'
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue'
import { useI18n } from 'vue-i18n'
import maplibregl from 'maplibre-gl'
import 'maplibre-gl/dist/maplibre-gl.css'

import { config } from '@/common/config'
import { fileInfoRevision } from '@/common/fileInfoRefresh'
import { getMapTheme } from '@/common/mapProviders'
import { buildGlobeStyle, globeCellsBounds, gpsCellKey } from '@/common/globeMap'
import { getGpsMapPoints, getQueryFiles } from '@/common/api'
import { getThumbUrl } from '@/common/utils'
import { useUIStore } from '@/stores/uiStore'
import { IconMapCenter, IconMapDefault, IconMapSatellite, IconZoomIn, IconZoomOut } from '@/common/icons'
import TButton from '@/components/TButton.vue'

// Location-view counterpart to PhotoMapView: the same photo points rendered
// on a 3D globe that flattens into a regular map as you zoom in.
const props = defineProps({
  queryParams: { type: Object, default: null },
  // Narrower query (the sidebar's selected place) to frame; null frames everything.
  focusParams: { type: Object, default: null },
  selectedFile: { type: Object, default: null },
  restoreView: { type: Object, default: null },
  active: { type: Boolean, default: true },
})
const emit = defineEmits(['open-cluster', 'select-file', 'preview-file', 'restored'])

const SOURCE_ID = 'photos'
const CLUSTER_RADIUS = 60

const { t } = useI18n()
const uiStore = useUIStore()
const mapEl = ref(null)
const loading = ref(true)
const points = ref([])
const zoom = ref(1.5)
const activeMaxZoom = ref(19)
const totalCount = computed(() => points.value.reduce((sum, point) => sum + point.count, 0))

let map = null
let resizeObserver = null
let markers = new Map()
let rankToFileId = new Map()
let pointRequestToken = 0
let openRequestToken = 0
let markerFrame = null
let needsRefresh = false
let focusRequestToken = 0

onMounted(() => {
  const theme = currentTheme()
  activeMaxZoom.value = theme.maxZoom
  map = new maplibregl.Map({
    container: mapEl.value,
    style: buildGlobeStyle(theme),
    center: [0, 20],
    zoom: zoom.value,
    maxZoom: theme.maxZoom,
    attributionControl: { compact: true },
    keyboard: false,
    dragRotate: false,
    pitchWithRotate: false,
    renderWorldCopies: false,
  })
  map.touchZoomRotate.disableRotation()
  map.on('style.load', addPhotoSource)
  map.on('render', scheduleMarkerUpdate)
  map.on('zoom', () => { zoom.value = map.getZoom() })
  resizeObserver = new ResizeObserver(() => map?.resize())
  resizeObserver.observe(mapEl.value)
  window.addEventListener('keydown', handleMapKeyDown, true)
  if (props.active) void loadPoints(true)
  else loading.value = false
})

onBeforeUnmount(() => {
  uiStore.setMapActive(false)
  window.removeEventListener('keydown', handleMapKeyDown, true)
  resizeObserver?.disconnect()
  pointRequestToken++
  openRequestToken++
  if (markerFrame !== null) cancelAnimationFrame(markerFrame)
  clearMarkers()
  map?.remove()
  map = null
})

watch(() => [config.settings.mapTheme, config.settings.mapProvider, config.settings.tiandituToken], () => {
  if (!map) return
  const theme = currentTheme()
  activeMaxZoom.value = theme.maxZoom
  map.setMaxZoom(theme.maxZoom)
  // setStyle drops the photo source; style.load re-adds it.
  map.setStyle(buildGlobeStyle(theme))
})
watch(() => [config.settings.mapMarkerSize, config.settings.thumbnailSize], () => {
  clearMarkers()
  scheduleMarkerUpdate()
})
watch(() => [props.selectedFile?.id, props.selectedFile?.gps_latitude, props.selectedFile?.gps_longitude], updateSelection)
// Compare serialized params so recomputed-but-equal objects don't reload the globe.
watch(() => JSON.stringify(props.queryParams), () => {
  if (!props.active) { needsRefresh = true; return }
  needsRefresh = false
  void loadPoints(true)
})
// Catalog edits (e.g. a changed location) move markers without changing the query.
watch(fileInfoRevision, () => {
  if (!props.active) { needsRefresh = true; return }
  void loadPoints(false)
})
watch(() => JSON.stringify(props.focusParams), () => {
  if (!props.active) { needsRefresh = true; return }
  void frameView(true)
})
watch(() => props.active, (active) => {
  if (!active) {
    pointRequestToken++
    openRequestToken++
    return
  }
  void nextTick(() => {
    if (!map || !props.active) return
    map.resize()
    if (props.restoreView || needsRefresh) {
      needsRefresh = false
      void loadPoints(true)
      return
    }
    scheduleMarkerUpdate()
  })
})

function currentTheme() {
  return getMapTheme(config.settings.mapProvider, config.settings.tiandituToken, config.settings.mapTheme)
}

async function loadPoints(fitToResults) {
  if (!map) return
  const token = ++pointRequestToken
  openRequestToken++
  loading.value = true
  try {
    const result = props.queryParams ? await getGpsMapPoints(props.queryParams) : []
    if (token !== pointRequestToken || !map || !props.active) return
    points.value = (result || []).filter(point => Number.isFinite(point.lat) && Number.isFinite(point.lon))
    rankToFileId = new Map(points.value.map(point => [Number(point.rank), Number(point.file_id)]))
    clearMarkers()
    setSourceData()
    if (fitToResults) {
      const view = props.restoreView
      if (view) {
        map.jumpTo({ center: [view.lon, view.lat], zoom: view.zoom })
        emit('restored')
      } else {
        await frameView(false)
      }
    }
  } finally {
    if (token === pointRequestToken) loading.value = false
  }
}

function pointFeatures() {
  return {
    type: 'FeatureCollection',
    features: points.value.map(point => ({
      type: 'Feature',
      geometry: { type: 'Point', coordinates: [point.lon, point.lat] },
      properties: { file_id: Number(point.file_id), count: Number(point.count), rank: Number(point.rank) },
    })),
  }
}

function addPhotoSource() {
  if (!map || map.getSource(SOURCE_ID)) return
  map.addSource(SOURCE_ID, {
    type: 'geojson',
    data: pointFeatures(),
    cluster: true,
    clusterRadius: CLUSTER_RADIUS,
    clusterMaxZoom: Math.max(0, activeMaxZoom.value - 1),
    // Photos per cluster, and the best-ranked photo to use as its cover.
    clusterProperties: {
      photos: ['+', ['get', 'count']],
      rank: ['min', ['get', 'rank']],
    },
  })
  // querySourceFeatures only sees tiles for sources used by a layer.
  map.addLayer({ id: 'photos-anchor', type: 'circle', source: SOURCE_ID, paint: { 'circle-opacity': 0, 'circle-radius': 1 } })
  clearMarkers()
}

function setSourceData() {
  const source = map?.getSource(SOURCE_ID)
  if (source) source.setData(pointFeatures())
}

function scheduleMarkerUpdate() {
  if (markerFrame !== null || !map) return
  markerFrame = requestAnimationFrame(() => {
    markerFrame = null
    updateMarkers()
  })
}

function updateMarkers() {
  if (!map || !props.active || !map.getSource(SOURCE_ID) || !map.isSourceLoaded(SOURCE_ID)) return
  const next = new Map()
  for (const feature of map.querySourceFeatures(SOURCE_ID)) {
    const props = feature.properties
    const isCluster = !!props.cluster
    const key = isCluster ? `c${props.cluster_id}` : `p${props.file_id}`
    if (next.has(key)) continue
    const [lon, lat] = feature.geometry.coordinates
    const marker = markers.get(key) || createMarker({
      lon, lat,
      clusterId: isCluster ? props.cluster_id : null,
      fileId: isCluster ? rankToFileId.get(Number(props.rank)) : Number(props.file_id),
      count: isCluster ? Number(props.photos) : Number(props.count),
    })
    if (marker) next.set(key, marker)
  }
  for (const [key, marker] of markers) {
    if (!next.has(key)) marker.remove()
  }
  markers = next
  updateSelection()
}

function createMarker(item) {
  if (!item.fileId) return null
  const size = Number(config.settings.mapMarkerSize || 64)
  const element = document.createElement('div')
  element.className = 'map-photo-marker-wrapper'
  element.style.width = `${size}px`
  element.style.height = `${size + 8}px`
  element.style.setProperty('--map-photo-marker-size', `${size}px`)
  element.innerHTML = `<div class="map-photo-marker"><img src="${getThumbUrl(item.fileId, false, config.settings.thumbnailSize || 512)}" />${item.count > 1 ? `<span>${item.count > 999 ? '999+' : item.count}</span>` : ''}</div>`
  const image = element.querySelector('img')
  const show = () => element.classList.add('map-photo-marker-visible')
  if (!image || image.complete) show()
  else {
    image.addEventListener('load', show, { once: true })
    image.addEventListener('error', show, { once: true })
  }
  element.addEventListener('click', (event) => {
    event.stopPropagation()
    void onMarkerClick(item)
  })
  element.addEventListener('dblclick', (event) => {
    event.stopPropagation()
    if (item.clusterId === null && item.count === 1) emit('preview-file', item.fileId, () => openSingle(item))
  })
  const marker = new maplibregl.Marker({ element, anchor: 'bottom', opacityWhenCovered: '0' })
    .setLngLat([item.lon, item.lat])
    .addTo(map)
  marker.photoItem = item
  return marker
}

async function onMarkerClick(item) {
  if (!map) return
  const token = ++openRequestToken
  if (item.clusterId === null && item.count === 1) {
    // The globe shows every located photo, so the photo may be outside the current
    // list (e.g. a sidebar location is selected); the parent then opens it on its own.
    emit('select-file', item.fileId, () => openSingle(item))
    return
  }
  let cellPoints
  if (item.clusterId !== null) {
    const source = map.getSource(SOURCE_ID)
    const expansionZoom = await source.getClusterExpansionZoom(item.clusterId)
    if (token !== openRequestToken || !map) return
    // Fly in while the cluster can still split; open it once it can't.
    if (expansionZoom <= activeMaxZoom.value && expansionZoom > map.getZoom()) {
      map.easeTo({ center: [item.lon, item.lat], zoom: expansionZoom, duration: 600 })
      return
    }
    const leaves = await source.getClusterLeaves(item.clusterId, Infinity, 0)
    if (token !== openRequestToken || !map) return
    cellPoints = leaves.map(leaf => ({
      lat: leaf.geometry.coordinates[1], lon: leaf.geometry.coordinates[0],
      file_id: Number(leaf.properties.file_id), rank: Number(leaf.properties.rank),
    }))
  } else {
    cellPoints = [{ lat: item.lat, lon: item.lon, file_id: item.fileId, rank: 0 }]
  }
  await openCells(cellPoints, item, token)
}

function openSingle(item) {
  if (!map) return
  const token = ++openRequestToken
  return openCells([{ lat: item.lat, lon: item.lon, file_id: item.fileId, rank: 0 }], item, token)
}

// Resolve the photos behind a set of 0.01° GPS cells, matching the backend grouping.
async function openCells(cellPoints, item, token) {
  const view = { lat: map.getCenter().lat, lon: map.getCenter().lng, zoom: map.getZoom() }
  const bounds = globeCellsBounds(cellPoints)
  const cells = new Set(cellPoints.map(point => gpsCellKey(point.lat, point.lon)))
  const files = await getQueryFiles({
    ...props.queryParams,
    gpsMinLat: bounds.minLat, gpsMaxLat: bounds.maxLat,
    gpsMinLon: bounds.minLon, gpsMaxLon: bounds.maxLon,
  }, 0, 0)
  if (!files || token !== openRequestToken || !map || !props.active) return
  const ids = files.filter(file => file.gps_latitude != null && file.gps_longitude != null
    && cells.has(gpsCellKey(Number(file.gps_latitude), Number(file.gps_longitude)))).map(file => Number(file.id))
  const coverId = Number(item.fileId)
  const fileIds = ids.includes(coverId) ? [coverId, ...ids.filter(id => id !== coverId)] : ids
  if (fileIds.length === 0) return
  emit('open-cluster', { ...bounds, file_id: fileIds[0], count: fileIds.length, fileIds, view })
}

function updateSelection() {
  const file = props.selectedFile
  const fileId = Number(file?.id || 0)
  const cell = file?.gps_latitude != null && file?.gps_longitude != null
    ? gpsCellKey(Number(file.gps_latitude), Number(file.gps_longitude)) : null
  for (const marker of markers.values()) {
    const item = marker.photoItem
    const selected = fileId > 0 && item.clusterId === null
      && (item.fileId === fileId || (cell !== null && gpsCellKey(item.lat, item.lon) === cell))
    marker.getElement().classList.toggle('map-photo-marker-selected', selected)
  }
}

function clearMarkers() {
  for (const marker of markers.values()) marker.remove()
  markers = new Map()
}

// Frame the selected place if there is one, otherwise all photos.
async function frameView(animate) {
  const token = ++focusRequestToken
  if (!props.focusParams) { fitToPoints(animate); return }
  const focusPoints = await getGpsMapPoints(props.focusParams)
  if (token !== focusRequestToken || !map) return
  fitToPoints(animate, (focusPoints || []).filter(point => Number.isFinite(point.lat) && Number.isFinite(point.lon)))
}

function fitToPoints(animate, framePoints = points.value) {
  if (!map) return
  if (framePoints.length === 0) {
    map.easeTo({ center: [0, 20], zoom: 1.5, duration: animate ? 600 : 0 })
    return
  }
  const bounds = new maplibregl.LngLatBounds()
  for (const point of framePoints) bounds.extend([point.lon, point.lat])
  map.fitBounds(bounds, {
    padding: 60,
    maxZoom: framePoints.length === 1 ? Math.min(MAP_SINGLE_POINT_FIT_ZOOM, activeMaxZoom.value) : activeMaxZoom.value,
    duration: animate ? 800 : 0,
  })
}

function zoomIn() { if (map && zoom.value < activeMaxZoom.value) map.zoomIn() }
function zoomOut() { if (map && zoom.value > 0) map.zoomOut() }
function handleMapKeyDown(event) {
  const target = event.target
  if (target?.tagName === 'INPUT' || target?.tagName === 'TEXTAREA' || target?.isContentEditable) return
  if (!uiStore.mapActive || event.metaKey || event.ctrlKey || event.altKey) return
  if (event.key === '=') { event.preventDefault(); zoomIn() }
  if (event.key === '-') { event.preventDefault(); zoomOut() }
}
</script>

<style scoped>
.photo-globe-view {
  background: radial-gradient(circle at center, color-mix(in srgb, var(--color-base-300) 70%, black) 0%, color-mix(in srgb, var(--color-base-300) 40%, black) 100%);
}

:deep(.maplibregl-marker-covered) {
  pointer-events: none;
}

:deep(.map-photo-marker-selected .map-photo-marker img) {
  border-color: var(--color-primary);
}

:deep(.map-photo-marker-selected .map-photo-marker::after) {
  border-top-color: var(--color-primary);
}
</style>
