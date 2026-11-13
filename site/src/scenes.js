// The scenes that have a live demo: the geographic ones, where a basemap underneath means
// something. `examples/web-mln/build.sh` copies the same list out of `examples/json`, so
// the two have to agree.
export const scenes = [
  {
    slug: 'san-francisco',
    title: 'San Francisco',
    poster: 'san-francisco.png',
    alt: 'Arcs and extruded blocks over San Francisco',
    blurb: 'Arcs, a `ScatterplotLayer` and labels across the city.',
    text: 'Arcs, a ScatterplotLayer and labels across the city.',
  },
  {
    slug: 'vancouver-blocks',
    title: 'Vancouver blocks',
    poster: 'geojson-vancouver.png',
    alt: 'Vancouver blocks from GeoJSON',
    text: 'GeoJsonLayer: 4,600 extruded blocks read from GeoJSON.',
  },
  {
    slug: 'maplibre-arcs',
    title: 'Arcs between the towers',
    poster: 'maplibre-arcs.png',
    alt: 'Arcs weaving between downtown towers',
    text: 'The arcs that weave between the basemap’s towers, which is where the depth sharing shows.',
  },
  {
    slug: 'heathrow-flights',
    title: 'Heathrow departures',
    poster: 'json-heathrow-flights.png',
    alt: 'Flight paths out of Heathrow',
    text: 'LineLayer: a day of departures from Heathrow.',
  },
  {
    slug: 'shadows',
    title: 'Shadows over Vancouver',
    poster: 'shadows.png',
    alt: 'Extruded blocks casting shadows',
    text: 'Shadow maps from a low sun over Vancouver’s extruded blocks.',
  },
  {
    slug: 'data-filter',
    title: 'Filtering on the GPU',
    poster: 'data-filter.png',
    alt: 'Points filtered on the GPU',
    text: 'DataFilterExtension: points dropped on the GPU rather than rebuilt on the CPU.',
  },
  {
    slug: 'osm-tiles',
    title: 'Raster tiles',
    poster: 'osm-tiles.png',
    alt: 'Raster tiles of Copenhagen',
    text: 'TileLayer: a frustum culled quadtree loading raster tiles in the background.',
  },
  {
    slug: 'terrain',
    title: 'Terrain',
    poster: 'terrain.png',
    alt: 'Terrain from elevation tiles',
    text: 'TerrainLayer: elevation tiles triangulated with Martini, lit by a directional light.',
  },
]
