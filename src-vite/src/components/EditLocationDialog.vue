<template>
  <ModalDialog :title="$t('file_info.edit_location')" :width="560" position-key="edit-location" @cancel="close">
    <form class="flex flex-col gap-3" @submit.prevent="save" @keydown.esc.stop.prevent="close">
      <div class="relative">
        <input
          ref="searchInput"
          v-model="query"
          type="text"
          class="w-full input input-sm px-2 text-[12px]"
          :placeholder="$t('file_info.location_search_placeholder')"
          :disabled="saving"
          @keydown.down.prevent="moveHighlight(1)"
          @keydown.up.prevent="moveHighlight(-1)"
          @keydown.enter.prevent="pickHighlighted"
        />
        <ul
          v-if="showResults"
          class="absolute left-0 right-0 top-full mt-1 z-[700] max-h-56 overflow-y-auto rounded-box border border-base-content/10 bg-base-100 shadow-lg text-[12px]"
        >
          <li v-if="!results.length" class="px-2 py-1.5 text-base-content/50">{{ $t('file_info.location_no_results') }}</li>
          <li
            v-for="(place, index) in results"
            :key="`${place.lat},${place.lon}`"
            :class="['px-2 py-1.5 cursor-pointer', index === highlighted ? 'bg-base-content/10' : 'hover:bg-base-content/5']"
            @mousedown.prevent="pickPlace(place)"
            @mouseenter="highlighted = index"
          >
            <span class="text-base-content">{{ place.name }}</span>
            <span class="text-base-content/50">{{ placeContext(place) }}</span>
          </li>
        </ul>
      </div>

      <div class="h-72 rounded-box overflow-hidden relative z-0">
        <MapView :lat="latitude" :lon="longitude" :zoom="11" pickable :label="selectedLabel || 'Lap'" @pick="pickPoint" />
      </div>

      <div class="flex items-center justify-between gap-2 text-[12px] min-h-6">
        <span v-if="latitude !== null && longitude !== null" class="text-base-content/70 truncate">
          <span v-if="selectedLabel">{{ selectedLabel }} · </span>{{ formatGpsCoordinates(latitude, longitude) }}
        </span>
        <span v-else class="text-base-content/40">{{ $t('file_info.location_none') }}</span>
        <span class="shrink-0 text-[11px] text-base-content/30">{{ $t('file_info.location_map_hint') }}</span>
      </div>

      <div class="flex items-center gap-2">
        <button
          v-if="hasExistingLocation"
          type="button"
          class="t-button-error"
          :disabled="saving"
          @click="remove"
        >{{ $t('file_info.location_remove') }}</button>
        <div class="flex-1"></div>
        <button type="button" class="t-button-default" :disabled="saving" @click="close">{{ $t('msgbox.cancel') }}</button>
        <button type="submit" class="t-button-primary" :disabled="saving || !changed">{{ $t('msgbox.save') }}</button>
      </div>
    </form>
  </ModalDialog>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import ModalDialog from './ModalDialog.vue';
import MapView from './MapView.vue';
import { libConfig } from '@/common/config';
import { searchPlaces, setFileLocation } from '@/common/api';
import { formatGpsCoordinates } from '@/common/utils';
import { useUIStore } from '@/stores/uiStore';
import { useToast } from '@/common/toast';

type Place = { name: string; admin1: string; admin2: string; cc: string; lat: number; lon: number };

const props = defineProps<{ files: Record<string, any>[] }>();
const emit = defineEmits<{
  (e: 'saved', files: Record<string, any>[], changes: Record<string, any>): void;
  (e: 'cancel'): void;
}>();

const { t } = useI18n();
const ui = useUIStore();
const toast = useToast();
const libraryId = libConfig._libraryId;
const files = [...props.files];
const first = files[0] ?? {};
const coordinate = (value: unknown) => (value === null || value === undefined || value === '' ? null : Number(value));
const sameLocation = files.every(file => file.gps_latitude === first.gps_latitude && file.gps_longitude === first.gps_longitude);
const initialLatitude = sameLocation ? coordinate(first.gps_latitude) : null;
const initialLongitude = sameLocation ? coordinate(first.gps_longitude) : null;

const latitude = ref<number | null>(initialLatitude);
const longitude = ref<number | null>(initialLongitude);
const selectedLabel = ref(sameLocation ? [first.geo_name, first.geo_admin1, first.geo_cc].filter(Boolean).join(', ') : '');
const query = ref('');
const results = ref<Place[]>([]);
const searched = ref(false);
const highlighted = ref(0);
const saving = ref(false);
const searchInput = ref<HTMLInputElement | null>(null);

const hasExistingLocation = files.some(file => coordinate(file.gps_latitude) !== null);
const changed = computed(() => latitude.value !== null && longitude.value !== null
  && (!sameLocation || latitude.value !== initialLatitude || longitude.value !== initialLongitude));
const showResults = computed(() => searched.value && query.value.trim() !== '' && parseCoordinates(query.value) === null);

// Accepts "36.16, -86.78" or "36.16 -86.78" as pasted from most map apps.
function parseCoordinates(text: string): [number, number] | null {
  const match = text.trim().match(/^(-?\d+(?:\.\d+)?)\s*[,\s]\s*(-?\d+(?:\.\d+)?)$/);
  if (!match) return null;
  const lat = Number(match[1]);
  const lon = Number(match[2]);
  return Math.abs(lat) <= 90 && Math.abs(lon) <= 180 ? [lat, lon] : null;
}

function placeContext(place: Place) {
  const parts = [place.admin1, place.cc].filter(part => part && part !== place.name);
  return parts.length ? `, ${parts.join(', ')}` : '';
}

let searchTimer: ReturnType<typeof setTimeout> | undefined;
let searchSeq = 0;
watch(query, text => {
  clearTimeout(searchTimer);
  const point = parseCoordinates(text);
  if (point) {
    [latitude.value, longitude.value] = point;
    selectedLabel.value = '';
    return;
  }
  if (!text.trim()) {
    results.value = [];
    searched.value = false;
    return;
  }
  searchTimer = setTimeout(async () => {
    const seq = ++searchSeq;
    const found = await searchPlaces(text, 20).catch(() => []);
    if (seq !== searchSeq) return;
    results.value = found as Place[];
    highlighted.value = 0;
    searched.value = true;
  }, 150);
});

function moveHighlight(step: number) {
  if (!results.value.length) return;
  highlighted.value = (highlighted.value + step + results.value.length) % results.value.length;
}

function pickHighlighted() {
  if (showResults.value && results.value[highlighted.value]) pickPlace(results.value[highlighted.value]);
  else if (changed.value) void save();
}

function pickPlace(place: Place) {
  latitude.value = place.lat;
  longitude.value = place.lon;
  selectedLabel.value = `${place.name}${placeContext(place)}`;
  searched.value = false;
}

function pickPoint({ lat, lon }: { lat: number; lon: number }) {
  latitude.value = Math.round(lat * 1e6) / 1e6;
  longitude.value = Math.round(lon * 1e6) / 1e6;
  selectedLabel.value = '';
}

async function persist(lat: number | null, lon: number | null) {
  if (saving.value) return;
  saving.value = true;
  try {
    const location = await setFileLocation(libraryId, files.map(file => Number(file.id)), lat, lon);
    if (libraryId !== libConfig._libraryId) {
      emit('cancel');
      return;
    }
    emit('saved', files, location);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    toast.error(`${t('file_info.location_save_failed')}: ${reason}`);
  } finally {
    saving.value = false;
  }
}

function save() {
  if (changed.value) return persist(latitude.value, longitude.value);
}

function remove() {
  return persist(null, null);
}

function close() {
  if (!saving.value) emit('cancel');
}

onMounted(async () => {
  ui.pushInputHandler('EditLocationDialog');
  await nextTick();
  searchInput.value?.focus();
});
onBeforeUnmount(() => {
  clearTimeout(searchTimer);
  ui.removeInputHandler('EditLocationDialog');
});
</script>
