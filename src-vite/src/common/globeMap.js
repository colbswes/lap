// Helpers for the MapLibre globe in PhotoGlobeView.

// Expand Leaflet-style `{s}` subdomain URLs into MapLibre's explicit tile list.
export function expandTileUrls(url, subdomains) {
  if (!url.includes('{s}')) return [url]
  const hosts = subdomains ? [...String(subdomains)] : ['a', 'b', 'c']
  return hosts.map(host => url.replace('{s}', host))
}

// Raster style for a mapProviders theme, on a globe with an atmosphere that
// fades out as the view zooms toward a flat map.
export function buildGlobeStyle(theme) {
  const sources = {}
  const layers = []
  theme.layers.forEach((layer, index) => {
    const id = `tiles-${index}`
    sources[id] = {
      type: 'raster',
      tiles: expandTileUrls(layer.url, layer.subdomains),
      tileSize: 256,
      maxzoom: theme.maxZoom,
      attribution: layer.attribution,
    }
    layers.push({ id, type: 'raster', source: id })
  })
  return {
    version: 8,
    projection: { type: 'globe' },
    sky: {
      'atmosphere-blend': ['interpolate', ['linear'], ['zoom'], 0, 1, 5, 1, 7, 0],
    },
    sources,
    layers,
  }
}

// Matches the backend's SQLite ROUND(x, 2) grouping (half away from zero).
function cellIndex(value) {
  return Math.sign(value) * Math.floor(Math.abs(value) * 100 + 0.5)
}

export function gpsCellKey(lat, lon) {
  return `${cellIndex(lat)}:${cellIndex(lon)}`
}

// Bounding box that fully covers every 0.01° cell the points fall in.
export function globeCellsBounds(points) {
  let minLat = Infinity, maxLat = -Infinity, minLon = Infinity, maxLon = -Infinity
  for (const point of points) {
    const lat = cellIndex(point.lat)
    const lon = cellIndex(point.lon)
    minLat = Math.min(minLat, lat)
    maxLat = Math.max(maxLat, lat)
    minLon = Math.min(minLon, lon)
    maxLon = Math.max(maxLon, lon)
  }
  const edge = 0.0051
  return {
    minLat: minLat / 100 - edge, maxLat: maxLat / 100 + edge,
    minLon: minLon / 100 - edge, maxLon: maxLon / 100 + edge,
  }
}
