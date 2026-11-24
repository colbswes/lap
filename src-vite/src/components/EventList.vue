<template>
  <div class="sidebar-panel">
    <div class="sidebar-panel-header">
      <span class="sidebar-panel-header-title flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">
        {{ props.titlebar }}<template v-if="events.length > 0"> ({{ events.length.toLocaleString() }})</template>
      </span>
      <ContextMenu :iconMenu="IconAdd" :menuItems="newEventMenuItems" :smallIcon="true" :title="$t('event.add')" />
    </div>

    <div v-if="events.length > 10 || concertArtists.length > 10" class="mx-1 mb-2 px-1 shrink-0">
      <div
        :class="[
          'h-8 flex items-center rounded-box transition-colors bg-base-100/40',
          isSearchFocused ? 'border-2 border-primary' : 'border border-base-content/10 hover:border-base-content/30',
        ]"
      >
        <IconSearch class="ml-2 w-4 h-4 shrink-0" :class="isSearchFocused ? 'text-primary/70' : 'text-base-content/30'" />
        <input
          v-model="searchQuery"
          type="text"
          :placeholder="$t('event.search')"
          class="w-full min-w-0 bg-transparent border-none focus:ring-0 px-2 text-sm placeholder-base-content/30 focus:outline-none"
          maxlength="255"
          @focus="isSearchFocused = true"
          @blur="isSearchFocused = false"
        />
        <button v-if="searchQuery" type="button" class="mr-1 p-1 rounded-box text-base-content/30 hover:text-base-content/70" @click="searchQuery = ''">
          <IconClose class="w-4 h-4" />
        </button>
      </div>
    </div>

    <div class="flex-1 overflow-x-hidden overflow-y-auto px-1 pb-1">
      <template v-for="kind in kinds" :key="kind">
        <button v-if="kind !== 'other' || events.some(e => e.kind === kind)" type="button" class="sidebar-item sidebar-item-hover w-full" :aria-expanded="expanded === kind" @click="expanded = expanded === kind ? null : kind">
          <component :is="collectionKindIcon(kind)" class="w-5 h-5 shrink-0" />
          <span class="sidebar-item-label">{{ t('event.' + categoryKeys[kind]) }}</span>
          <span class="ml-auto text-base-content/40" aria-hidden="true">{{ expanded === kind ? '▾' : '▸' }}</span>
        </button>
        <div v-if="expanded === kind || searchQuery.trim()" class="pl-3">
          <template v-if="kind === 'concert'">
            <button v-for="artist in visibleArtists(concertArtists)" :key="artist.id" class="sidebar-item w-full" :class="!selectedId && artist.id === libConfig.event.artistId ? 'sidebar-item-selected' : 'sidebar-item-hover'" @click="selectArtist(artist, null)">
              <span class="sidebar-item-label">{{ artist.name || t('event.unassigned') }}</span>
              <span class="sidebar-item-count ml-auto">{{ artist.count }}</span>
            </button>
          </template>
          <component v-if="visibleEvents(kind).length" :is="kind === 'concert' ? 'details' : 'div'" :open="!!searchQuery || !!selectedId && events.some(e => e.id === selectedId && e.kind === kind)">
            <summary v-if="kind === 'concert' && visibleEvents(kind).length" class="cursor-pointer px-2 py-1 text-sm">{{ t('event.by_show') }}</summary>
            <template v-for="event in visibleEvents(kind)" :key="event.id">
              <div
                :data-collection-drop-id="renamingId === event.id ? undefined : event.id"
                :class="[
                  'sidebar-item group border-2 border-transparent transition-colors',
                  selectedId === event.id ? 'sidebar-item-selected' : 'sidebar-item-hover',
                ]"
                @click="selectEvent(event)"
              >
                <component :is="collectionKindIcon(event.kind)" class="pr-1 w-6 h-6 shrink-0" />
                <input
                  v-if="renamingId === event.id"
                  ref="renameInputRef"
                  v-model="renameValue"
                  class="input px-1 min-w-0 flex-1 text-base"
                  maxlength="64"
                  @click.stop
                  @mousedown.stop
                  @keydown.stop
                  @keydown.enter.prevent="commitRename(event)"
                  @keydown.escape.prevent="cancelRename"
                  @blur="commitRename(event)"
                />
                <div v-else class="min-w-0 flex-1 flex flex-col leading-tight">
                  <span class="sidebar-item-label">{{ event.name }}</span>
                  <span v-if="event.dateLabel" class="text-[11px] text-base-content/40 truncate">{{ event.dateLabel }}</span>
                </div>
                <div v-if="renamingId !== event.id" class="ml-auto flex flex-row items-center text-base-content/30">
                  <span v-if="event.count > 0" class="sidebar-item-count shrink-0">{{ event.count.toLocaleString() }}</span>
                  <div :class="selectedId === event.id ? '' : 'hidden group-hover:block'">
                    <ContextMenu :iconMenu="IconMore" :menuItems="eventMenuItems(event)" :smallIcon="true" />
                  </div>
                </div>
              </div>
              <div v-if="event.kind === 'festival' && selectedId === event.id" class="pl-3">
                <button v-for="artist in festivalArtists" :key="artist.id" class="sidebar-item w-full" :class="artist.id === libConfig.event.artistId ? 'sidebar-item-selected' : 'sidebar-item-hover'" @click="selectArtist(artist, event.id)">
                  <span class="sidebar-item-label">{{ artist.name || t('event.unassigned') }}</span>
                  <span class="sidebar-item-count ml-auto">{{ artist.count }}</span>
                </button>
              </div>
            </template>
          </component>
        </div>
      </template>
      <p v-if="!events.length && !isLoading" class="px-2 py-3 text-sm text-base-content/50">{{ t('event.empty_hint') }}</p>
    </div>

    <MessageBox
      v-if="deleteTarget"
      :title="$t('collection.delete_confirm_title')"
      :message="$t('collection.delete_confirm_message', { name: deleteTarget.name })"
      :OkText="$t('collection.delete_confirm_ok')"
      :cancelText="$t('msgbox.cancel')"
      :warningOk="true"
      @ok="confirmDelete"
      @cancel="deleteTarget = null"
    />
  </div>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { emit as tauriEmit, listen } from '@tauri-apps/api/event';
import { useI18n } from 'vue-i18n';
import { libConfig } from '@/common/config';
import { createEvent, deleteCollection, listEventArtists, listEvents, renameCollection, setCollectionKind } from '@/common/api';
import { collectionKindIcon } from '@/common/collectionKinds';
import { IconAdd, IconClose, IconCollection, IconFestival, IconMore, IconMusic, IconOk, IconRename, IconSearch, IconTrash } from '@/common/icons';
import ContextMenu from '@/components/ContextMenu.vue';
import MessageBox from '@/components/MessageBox.vue';

const props = defineProps({
  titlebar: { type: String, default: '' },
});

const kinds = ['concert', 'festival', 'other'] as const;
type EventKind = typeof kinds[number];
const categoryKeys = { concert: 'concerts', festival: 'festivals', other: 'other' };
const expanded = ref<EventKind | null>(libConfig.event.artistId ? 'concert' : null);
type Artist = { id: number; name: string; count: number };
const concertArtists = ref<Artist[]>([]);
const festivalArtists = ref<Artist[]>([]);
type EventItem = {
  id: number;
  name: string;
  kind: EventKind;
  count: number;
  startDate: number | null;
  endDate: number | null;
  dateLabel: string;
};

const { t, locale } = useI18n();
const events = ref<EventItem[]>([]);
const isLoading = ref(true);
const searchQuery = ref('');
const isSearchFocused = ref(false);
const selectedId = computed(() => Number(libConfig.event.selectedId || 0) || null);
const renamingId = ref<number | null>(null);
const renameValue = ref('');
const renameInputRef = ref<HTMLInputElement | HTMLInputElement[] | null>(null);
const deleteTarget = ref<EventItem | null>(null);
let loadRequest = 0;
const unlisteners: Array<() => void> = [];
let mounted = true;

function formatDateRange(start: number | null, end: number | null) {
  if (!start) return '';
  const startDate = new Date(start * 1000);
  const endDate = new Date((end || start) * 1000);
  const day = new Intl.DateTimeFormat(locale.value, { month: 'short', day: 'numeric' });
  const sameDay = startDate.toDateString() === endDate.toDateString();
  // Footage past midnight still belongs to the same night.
  const overnight = !sameDay && (endDate.getTime() - startDate.getTime()) < 18 * 3600 * 1000;
  return sameDay || overnight ? day.format(startDate) : `${day.format(startDate)} – ${day.format(endDate)}`;
}

function matches(name: string) {
  return name.toLocaleLowerCase().includes(searchQuery.value.trim().toLocaleLowerCase());
}
function visibleEvents(kind: EventKind) {
  return events.value.filter(event => event.kind === kind && matches(event.name));
}
function visibleArtists(artists: Artist[]) {
  return artists.filter(artist => matches(artist.name || t('event.unassigned')));
}
function selectArtist(artist: Artist, eventId: number | null) {
  libConfig.activePane = 'main';
  Object.assign(libConfig.event, { selectedId: eventId, artistId: artist.id, artistName: artist.name });
  libConfig.event.activateTick++;
}
watch(() => events.value.find(e => e.id === selectedId.value), async (event, previous, onCleanup) => {
  if (event && (event.id !== previous?.id || event.kind !== previous?.kind)) expanded.value = event.kind;
  let active = true;
  onCleanup(() => { active = false; });
  festivalArtists.value = [];
  if (event?.kind === 'festival') {
    const artists = await listEventArtists(null, event.id);
    if (active) festivalArtists.value = artists;
  }
});

async function loadEvents() {
  const request = ++loadRequest;
  const libraryId = libConfig._libraryId;
  const [result, artists] = await Promise.all([listEvents(), listEventArtists('concert')]);
  if (!mounted || request !== loadRequest || libraryId !== libConfig._libraryId) return;
  concertArtists.value = artists;
  events.value = Array.isArray(result)
    ? result.map((item: any) => ({
      id: Number(item.id),
      name: String(item.name || ''),
      kind: item.kind,
      count: Number(item.count || 0),
      startDate: item.startDate ?? null,
      endDate: item.endDate ?? null,
      dateLabel: formatDateRange(item.startDate ?? null, item.endDate ?? null),
    }))
    : [];
  if (selectedId.value && !events.value.some(event => event.id === selectedId.value)) {
    Object.assign(libConfig.event, { selectedId: null, artistId: 0, artistName: '' });
  }
}

function selectEvent(event: EventItem) {
  libConfig.activePane = 'main';
  expanded.value = event.kind;
  Object.assign(libConfig.event, { selectedId: event.id, artistId: 0, artistName: '' });
  libConfig.event.activateTick = Number(libConfig.event.activateTick || 0) + 1;
}

async function addEvent(kind: EventKind) {
  const sameKind = events.value.filter(event => event.kind === kind).length;
  const name = t(`event.default_${kind}_name`, { index: sameKind + 1 });
  const created = await createEvent(name, kind);
  if (!created?.id) return;
  await loadEvents();
  const event = events.value.find(item => item.id === Number(created.id));
  if (event) {
    selectEvent(event);
    void startRename(event);
  }
}

const newEventMenuItems = computed(() => [
  { label: t('event.new_concert'), icon: IconMusic, action: () => { void addEvent('concert'); } },
  { label: t('event.new_other'), icon: IconCollection, action: () => { void addEvent('other'); } },
  { label: t('event.new_festival'), icon: IconFestival, action: () => { void addEvent('festival'); } },
]);

async function changeKind(event: EventItem, kind: EventKind | null) {
  if (event.kind === kind) return;
  if (await setCollectionKind(event.id, kind) == null) return;
  await tauriEmit('collections-changed', {});
}

function eventMenuItems(event: EventItem) {
  return [
    { label: t('collection.rename'), icon: IconRename, action: () => { void startRename(event); } },
    {
      label: t('collection.type'),
      icon: collectionKindIcon(event.kind),
      children: kinds.map(kind => ({
        label: t(`collection.type_${kind}`),
        icon: event.kind === kind ? IconOk : undefined,
        action: () => { void changeKind(event, kind); },
      })),
    },
    { label: t('event.make_collection'), icon: IconCollection, action: () => { void changeKind(event, null); } },
    { label: '-', action: null },
    { label: t('collection.delete'), icon: IconTrash, action: () => { deleteTarget.value = event; } },
  ];
}

async function startRename(event: EventItem) {
  renamingId.value = event.id;
  renameValue.value = event.name;
  await nextTick();
  const input = Array.isArray(renameInputRef.value) ? renameInputRef.value[0] : renameInputRef.value;
  input?.focus();
  input?.select();
}

function cancelRename() {
  renamingId.value = null;
  renameValue.value = '';
}

async function commitRename(event: EventItem) {
  if (renamingId.value !== event.id) return;
  const name = renameValue.value.trim();
  cancelRename();
  if (!name || name === event.name) return;
  if (await renameCollection(event.id, name) == null) return;
  await tauriEmit('collections-changed', {});
}

async function confirmDelete() {
  const target = deleteTarget.value;
  deleteTarget.value = null;
  if (!target) return;
  if (await deleteCollection(target.id) == null) return;
  if (selectedId.value === target.id) Object.assign(libConfig.event, { selectedId: null, artistId: 0, artistName: '' });
  await tauriEmit('collections-changed', {});
}

onMounted(async () => {
  try {
    await loadEvents();
  } finally {
    isLoading.value = false;
  }
  for (const name of ['collections-changed', 'collection-files-dropped', 'event-artists-changed', 'library-switched']) {
    const stop = await listen(name, () => { void loadEvents(); });
    if (mounted) unlisteners.push(stop); else stop();
  }
});

onBeforeUnmount(() => {
  mounted = false;
  loadRequest++;
  unlisteners.splice(0).forEach(stop => stop());
});
</script>
