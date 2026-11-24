<template>
  <div class="sidebar-panel">
    <div class="sidebar-panel-header">
      <span class="sidebar-panel-header-title flex-1">
        {{ $t('album.smart_album_list') }}<template v-if="customSmartAlbums.length > 0"> ({{ customSmartAlbums.length.toLocaleString() }})</template>
      </span>
      <TButton
        :icon="IconAdd"
        :buttonSize="'small'"
        :tooltip="$t('album.add_smart_album')"
        @click="clickAddSmartAlbum"
      />
      <ContextMenu
        data-smart-album-reorder-area
        :menuItems="smartAlbumPanelMenuItems"
        :iconMenu="IconMore"
        :smallIcon="true"
        @open-change="smartAlbumPanelMenuOpen = $event"
      />
    </div>

    <div class="mx-1 mb-2 px-1 shrink-0">
      <div
        :class="[
          'h-8 flex items-center rounded-box transition-colors bg-base-100/40',
          isSmartAlbumSearchFocused ? 'border-2 border-primary' : 'border border-base-content/10 hover:border-base-content/30',
          customSmartAlbums.length === 0 ? 'opacity-50' : '',
        ]"
      >
        <IconSearch class="ml-2 w-4 h-4 shrink-0" :class="isSmartAlbumSearchFocused ? 'text-primary/70' : 'text-base-content/30'" />
        <input
          v-model="smartAlbumSearch"
          type="text"
          :disabled="customSmartAlbums.length === 0"
          :placeholder="$t('album.search_smart_albums')"
          class="w-full min-w-0 bg-transparent border-none focus:ring-0 px-2 text-sm placeholder-base-content/30 focus:outline-none disabled:opacity-50"
          @focus="isSmartAlbumSearchFocused = true"
          @blur="isSmartAlbumSearchFocused = false"
        />
        <button
          v-if="smartAlbumSearch"
          type="button"
          :disabled="customSmartAlbums.length === 0"
          class="mr-1 p-1 rounded-box text-base-content/30 hover:text-base-content/70 disabled:opacity-30"
          @click="smartAlbumSearch = ''"
        >
          <IconClose class="w-4 h-4" />
        </button>
      </div>
    </div>

    <div v-if="filteredSmartAlbums.length > 0" class="flex min-h-0 flex-1 transform-gpu overflow-hidden rounded-box">
      <VueDraggable
        v-model="customSmartAlbums"
        tag="ul"
        :disabled="!isReorderingSmartAlbums || !canReorderSmartAlbums"
        handle=".smart-album-drag-handle"
        direction="vertical"
        :group="{ name: 'smart-album-list', pull: false, put: false }"
        :force-fallback="true"
        ghost-class="sidebar-reorder-placeholder"
        :revertOnSpill="true"
        :bubble-scroll="false"
        :animation="150"
        @start="onDragStart"
        @end="onDragEnd"
        @drop.stop
        class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto select-none"
      >
        <li v-for="smartAlbum in filteredSmartAlbums" :key="smartAlbum.id">
          <div
            data-smart-album-reorder-area
            :class="[
              'sidebar-item sidebar-item-media group transition-colors',
              libConfig.smartAlbum.type === 'custom' && libConfig.smartAlbum.id === smartAlbum.id ? 'sidebar-item-selected' : 'sidebar-item-hover',
              isSmartAlbumHidden(smartAlbum.id) ? 'opacity-50' : '',
            ]"
            @click="clickCustomSmartAlbum(smartAlbum)"
            @contextmenu.prevent.stop="(event: MouseEvent) => handleSmartAlbumContextMenu(smartAlbum, event)"
          >
            <IconDragHandle
              v-if="isReorderingSmartAlbums && canReorderSmartAlbums"
              class="smart-album-drag-handle p-1 w-6 h-6 shrink-0 cursor-grab text-base-content/30"
              :title="$t('album.smart_edit.reorder')"
              @click.stop
            />
            <div class="w-10 h-10 mr-2 rounded-box shrink-0 overflow-hidden border border-base-content/5 bg-base-content/5">
              <img
                v-if="getSmartAlbumCount(smartAlbum) > 0 && getSmartAlbumCoverSrc(smartAlbum) && Number(smartAlbumCoverErrors[smartAlbum.id]) !== Number(smartAlbum.coverFileId)"
                :src="getSmartAlbumCoverSrc(smartAlbum)"
                class="w-full h-full object-cover"
                @error="smartAlbumCoverErrors[smartAlbum.id] = Number(smartAlbum.coverFileId)"
              />
              <div v-else class="w-full h-full flex items-center justify-center text-base-content/30">
                <IconError class="w-5 h-5" />
              </div>
            </div>
            <span class="sidebar-item-label">{{ smartAlbum.name }}</span>
            <div class="ml-auto flex flex-row items-center text-base-content/30">
              <span
                v-if="hasSmartAlbumCount(smartAlbum)"
                class="sidebar-item-count shrink-0"
              >{{ getSmartAlbumCount(smartAlbum).toLocaleString() }}</span>
              <button
                type="button"
                :title="isSmartAlbumHidden(smartAlbum.id) ? $t('album.smart_edit.show') : $t('album.smart_edit.hide')"
                :aria-label="isSmartAlbumHidden(smartAlbum.id) ? $t('album.smart_edit.show') : $t('album.smart_edit.hide')"
                :aria-pressed="isSmartAlbumHidden(smartAlbum.id)"
                :class="[
                  'p-1 rounded-box text-base-content/45 hover:text-base-content hover:bg-base-100/40 cursor-pointer shrink-0 transition-colors',
                  isSmartAlbumHidden(smartAlbum.id) ? 'flex' : 'hidden group-hover:flex',
                ]"
                @click.stop="toggleSmartAlbumHidden(smartAlbum)"
              >
                <component :is="isSmartAlbumHidden(smartAlbum.id) ? IconHide : IconUnhide" class="w-4 h-4" />
              </button>
              <div :class="[isSmartAlbumSelected(smartAlbum) ? '' : 'hidden group-hover:flex']">
                <ContextMenu
                  :ref="(element: any) => { if (element) smartAlbumContextMenus[smartAlbum.id] = element }"
                  :iconMenu="IconMore"
                  :menuItems="getMoreMenuItems(smartAlbum)"
                  :smallIcon="true"
                />
              </div>
            </div>
          </div>
        </li>
      </VueDraggable>
    </div>
    <div v-else-if="customSmartAlbums.length > 0" class="sidebar-empty text-sm">
      <span class="text-center">{{ $t('album.no_smart_albums_found') }}</span>
    </div>
    <div v-else class="mt-2 px-2 flex flex-col items-center justify-center text-base-content/30">
      <span class="text-sm text-center">{{ $t('album.no_smart_albums.description') }}</span>
    </div>

    <SmartAlbumEdit
      v-if="showEditDialog"
      :smartAlbum="editingSmartAlbum"
      @ok="saveSmartAlbum"
      @cancel="closeEditDialog"
    />
    <MessageBox
      v-if="showDeleteDialog"
      :title="$t('album.smart_edit.delete')"
      :message="$t('album.smart_edit.delete_message', { name: deletingSmartAlbum?.name || '' })"
      :OkText="$t('album.smart_edit.delete')"
      :cancelText="$t('msgbox.cancel')"
      warningOk
      @ok="confirmDeleteSmartAlbum"
      @cancel="closeDeleteDialog"
    />
  </div>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onBeforeUnmount } from 'vue';
import { useI18n } from 'vue-i18n';
import { emit as tauriEmit, listen } from '@tauri-apps/api/event';
import { VueDraggable, type DraggableEvent } from 'vue-draggable-plus';
import { config, libConfig } from '@/common/config';
import { useUIStore } from '@/stores/uiStore';
import { useToast } from '@/common/toast';
import { getThumbUrl, getThumbnailDataUrl, getThumbnailDataUrlInflight, isWin, setThumbnailDataUrlInflight } from '@/common/utils';
import { getFileThumbById, getSmartQueryCountAndSum, getSmartQueryFileIds } from '@/common/api';
import { isHiddenStringId } from '@/common/hiddenAlbums';
import { buildSmartDuplicateDraft, saveFilesAsNewCollection } from '@/common/collectionActions';
import { useLibraryChangeRefresh } from '@/composables/useLibraryChangeRefresh';
import { IconAdd, IconClose, IconCollection, IconCopy, IconDragHandle, IconEdit, IconHide, IconMore, IconError, IconOrder, IconSearch, IconTrash, IconUnhide } from '@/common/icons';
import TButton from '@/components/TButton.vue';
import ContextMenu from '@/components/ContextMenu.vue';
import SmartAlbumEdit from '@/components/SmartAlbumEdit.vue';
import MessageBox from '@/components/MessageBox.vue';

const props = defineProps<{
  titlebar?: string;
  initialDraft?: any;
}>();

const emit = defineEmits<{
  (e: 'draft-consumed'): void;
}>();

const customSmartAlbums = ref<any[]>([]);
const uiStore = useUIStore();
const toast = useToast();
const { t } = useI18n();
const showEditDialog = ref(false);
const editingSmartAlbum = ref<any | null>(null);
const showDeleteDialog = ref(false);
const deletingSmartAlbum = ref<any | null>(null);
const smartAlbumContextMenus = ref<Record<string, any>>({});
const smartAlbumCoverErrors = ref<Record<string, number>>({});
const smartAlbumCoverUrls = ref<Record<string, string>>({});
const isReorderingSmartAlbums = ref(false);
const smartAlbumPanelMenuOpen = ref(false);
const smartAlbumSearch = ref('');
const isSmartAlbumHidden = (smartAlbumId: string) =>
  isHiddenStringId(libConfig.hiddenSmartAlbumIds, String(smartAlbumId || ''));

const hiddenSmartAlbumCount = computed(() =>
  customSmartAlbums.value.filter(album => isSmartAlbumHidden(album.id)).length
);

async function toggleSmartAlbumHidden(smartAlbum: any) {
  await libConfig.toggleSmartAlbumHidden(String(smartAlbum.id));
  await tauriEmit('library-total-refreshed', { source: 'hidden-filters' });
}

async function showAllSmartAlbums() {
  await libConfig.showAllSmartAlbums();
  await tauriEmit('library-total-refreshed', { source: 'hidden-filters' });
}

const canReorderSmartAlbums = computed(() => customSmartAlbums.value.length > 1 && !smartAlbumSearch.value.trim());
const smartAlbumPanelMenuItems = computed(() => [
  {
    label: t('album.smart_edit.reorder'),
    icon: IconOrder,
    disabled: !canReorderSmartAlbums.value,
    action: () => { isReorderingSmartAlbums.value = !isReorderingSmartAlbums.value; },
  },
  ...(hiddenSmartAlbumCount.value > 0 ? [
    {
      label: t('album.smart_edit.show_all'),
      icon: IconUnhide,
      action: () => { void showAllSmartAlbums(); },
    },
  ] : []),
]);
const isSmartAlbumSearchFocused = ref(false);
let smartAlbumCoverLoadToken = 0;
let smartCountRequest = 0;
let smartCountsDisposed = false;
const unlistenSmartCounts: (() => void)[] = [];

const filteredSmartAlbums = computed(() => {
  const query = smartAlbumSearch.value.trim().toLowerCase();
  if (!query) return customSmartAlbums.value;
  return customSmartAlbums.value.filter(smartAlbum => smartAlbum.name.toLowerCase().includes(query));
});

watch(() => libConfig.smartAlbums, (albums) => {
  customSmartAlbums.value = Array.isArray(albums) ? [...albums] : [];
}, { immediate: true });

// Counts are computed per smart query. Re-fetch whenever the set of albums or
// their queries change (id/updatedAt), so each
// album shows its own count instead of the currently-viewed query's count.
const smartAlbumCountSignature = computed(() =>
  (libConfig.smartAlbums || []).map((album: any) => `${album.id}:${album.updatedAt ?? album.createdAt ?? ''}`).join('|')
);

async function refreshSmartAlbumCounts() {
  const request = ++smartCountRequest;
  const libraryId = libConfig._libraryId;
  const albums = customSmartAlbums.value.filter((album: any) => {
    const rules = Array.isArray(album?.query?.rules) ? album.query.rules : [];
    return rules.length > 0;
  });
  if (albums.length === 0) return;

  const results = await Promise.all(albums.map(async (album: any) => {
    const query = album.query;
    try {
      const params = {
        smartAlbumId: String(album.id),
        version: Number(query.version || 1),
        match: query.match === 'any' ? 'any' : 'all',
        rules: query.rules,
        sortType: Number(album?.sort?.type ?? 0),
        sortOrder: Number(album?.sort?.order ?? 1),
        folderSort: Number(config.settings.folderSort || 0),
        calendarSort: Number(config.settings.calendarSort || 0),
        // Count/sum query is not grouped, so category ordering is irrelevant here.
        categorySort: 0,
      };
      const result = await getSmartQueryCountAndSum(params);
      return [String(album.id), Number(result?.[0] || 0)] as const;
    } catch {
      return [String(album.id), 0] as const;
    }
  }));

  if (smartCountsDisposed || request !== smartCountRequest || libraryId !== libConfig._libraryId) return;
  const countMap = Object.fromEntries(results);
  libConfig.smartAlbums = (libConfig.smartAlbums || []).map((album: any) => ({
    ...album,
    count: countMap[String(album.id)] ?? 0,
  }));
}
useLibraryChangeRefresh(refreshSmartAlbumCounts);

watch(
  () => [smartAlbumCountSignature.value, libConfig._libraryId],
  () => { void refreshSmartAlbumCounts(); },
  { immediate: true },
);

watch(canReorderSmartAlbums, (canReorder) => {
  if (!canReorder) isReorderingSmartAlbums.value = false;
});

watch(() => libConfig._libraryId, () => {
  isReorderingSmartAlbums.value = false;
});

const getSmartAlbumCoverSrc = (smartAlbum: any) => {
  const coverFileId = Number(smartAlbum?.coverFileId || 0);
  if (!coverFileId) return '';
  return isWin ? smartAlbumCoverUrls.value[String(smartAlbum.id)] || '' : getThumbUrl(coverFileId, false, config.settings.thumbnailSize);
};

const loadWindowsSmartAlbumCovers = async (albums: any[]) => {
  if (!isWin) return;
  const loadToken = ++smartAlbumCoverLoadToken;

  const covers = await Promise.all(albums.map(async (smartAlbum) => {
    const coverFileId = Number(smartAlbum?.coverFileId || 0);
    if (!coverFileId) return [String(smartAlbum.id), ''] as const;

    const inflight = getThumbnailDataUrlInflight(coverFileId, config.settings.thumbnailSize);
    const url = await (inflight || setThumbnailDataUrlInflight(
      coverFileId,
      config.settings.thumbnailSize,
      getFileThumbById(coverFileId, config.settings.thumbnailSize, false)
        .then(thumb => getThumbnailDataUrl(thumb, '', false, config.settings.thumbnailSize))
    ));
    return [String(smartAlbum.id), url || getThumbUrl(coverFileId, false, config.settings.thumbnailSize)] as const;
  }));

  if (loadToken !== smartAlbumCoverLoadToken) return;
  smartAlbumCoverUrls.value = Object.fromEntries(covers.filter(([, url]) => url));
};

watch(
  () => customSmartAlbums.value.map((smartAlbum: any) => [smartAlbum.id, smartAlbum.coverFileId]),
  () => { void loadWindowsSmartAlbumCovers(customSmartAlbums.value); },
  { immediate: true },
);

function clickCustomSmartAlbum(smartAlbum: any) {
  libConfig.smartAlbum.type = 'custom';
  libConfig.smartAlbum.id = smartAlbum.id;
}

const isSmartAlbumSelected = (smartAlbum: any) => libConfig.smartAlbum.type === 'custom' && libConfig.smartAlbum.id === smartAlbum.id;
const getSmartAlbumCount = (smartAlbum: any) => {
  return Number(smartAlbum?.count || 0);
};
const hasSmartAlbumCount = (smartAlbum: any) => getSmartAlbumCount(smartAlbum) > 0;

function clickAddSmartAlbum() {
  editingSmartAlbum.value = null;
  showEditDialog.value = true;
}

function openNewSmartAlbumDraft(draft: any) {
  if (!draft) return;
  editingSmartAlbum.value = draft;
  showEditDialog.value = true;
  emit('draft-consumed');
}

defineExpose({ openNewSmartAlbumDraft });

watch(
  () => props.initialDraft,
  (draft) => {
    if (draft) openNewSmartAlbumDraft(draft);
  },
  { immediate: true },
);

async function saveSmartAlbumAsCollection(smartAlbum: any) {
  const query = smartAlbum?.query;
  const rules = Array.isArray(query?.rules) ? query.rules : [];
  if (!rules.length) return;
  try {
    const fileIds = await getSmartQueryFileIds({
      smartAlbumId: String(smartAlbum.id),
      version: Number(query.version || 1),
      match: query.match === 'any' ? 'any' : 'all',
      rules,
      favoritesOnly: false,
      fileType: 0,
      sortType: Number(smartAlbum?.sort?.type ?? 0),
      sortOrder: Number(smartAlbum?.sort?.order ?? 1),
      randomSeed: 0,
      folderSort: Number(config.settings.folderSort || 0),
      calendarSort: Number(config.settings.calendarSort || 0),
      categorySort: 0,
      groupBy: 0,
    });
    const created = await saveFilesAsNewCollection(smartAlbum.name, fileIds || []);
    if (created) {
      toast.success(t('collection.saved_as_collection_toast', {
        name: created.collection.name,
        count: created.addedCount.toLocaleString(),
      }));
    }
  } catch (error) {
    console.error('Failed to save smart collection as collection:', error);
  }
}

function duplicateSmartAlbum(smartAlbum: any) {
  openNewSmartAlbumDraft(buildSmartDuplicateDraft(smartAlbum, t('album.smart_edit.copy_suffix')));
}

function handleSmartAlbumContextMenu(smartAlbum: any, event: MouseEvent) {
  clickCustomSmartAlbum(smartAlbum);
  smartAlbumContextMenus.value[smartAlbum.id]?.open?.(event.clientX, event.clientY);
}

const getMoreMenuItems = (smartAlbum: any) => {
  const hidden = isSmartAlbumHidden(smartAlbum.id);
  return [
    {
      label: t('album.smart_edit.title_edit'),
      icon: IconEdit,
      action: () => clickEditSmartAlbum(smartAlbum),
    },
    {
      label: hidden ? t('album.smart_edit.show') : t('album.smart_edit.hide'),
      icon: hidden ? IconUnhide : IconHide,
      action: () => { void toggleSmartAlbumHidden(smartAlbum); },
    },
    {
      label: "-",
      action: null
    },
    {
      label: t('collection.save_as_collection'),
      icon: IconCollection,
      action: () => { void saveSmartAlbumAsCollection(smartAlbum); },
    },
    {
      label: t('album.smart_edit.duplicate'),
      icon: IconCopy,
      action: () => duplicateSmartAlbum(smartAlbum),
    },
    {
      label: "-",
      action: null
    },
    {
      label: t('album.smart_edit.delete'),
      icon: IconTrash,
      action: () => clickDeleteSmartAlbum(smartAlbum),
    },
  ];
};

function handleReorderOutsidePointerDown(event: PointerEvent) {
  if (event.button !== 0 || !isReorderingSmartAlbums.value || smartAlbumPanelMenuOpen.value) return;
  if (event.target instanceof Element && event.target.closest('[data-smart-album-reorder-area]')) return;
  isReorderingSmartAlbums.value = false;
}

onMounted(async () => {
  document.addEventListener('pointerdown', handleReorderOutsidePointerDown, true);
  for (const eventName of ['album-updated', 'index_finished']) {
    const stop = await listen(eventName, (event: any) => {
      if (eventName === 'album-updated' && !event.payload?.filtersChanged) return;
      void refreshSmartAlbumCounts();
    });
    if (smartCountsDisposed) { stop(); return; }
    unlistenSmartCounts.push(stop);
  }
});

onBeforeUnmount(() => {
  smartCountsDisposed = true;
  ++smartCountRequest;
  unlistenSmartCounts.forEach(stop => stop());
  document.removeEventListener('pointerdown', handleReorderOutsidePointerDown, true);
  uiStore.removeInputHandler('SmartAlbumListDrag');
});

function onDragStart() {
  uiStore.removeInputHandler('SmartAlbumListDrag');
  uiStore.pushInputHandler('SmartAlbumListDrag');
}

function onDragEnd({ oldIndex, newIndex }: DraggableEvent) {
  setTimeout(() => uiStore.removeInputHandler('SmartAlbumListDrag'), 0);
  if (oldIndex === newIndex) return;
  libConfig.smartAlbums = [...customSmartAlbums.value];
}

function clickEditSmartAlbum(smartAlbum: any) {
  editingSmartAlbum.value = smartAlbum;
  showEditDialog.value = true;
}

function closeEditDialog() {
  showEditDialog.value = false;
  editingSmartAlbum.value = null;
}

async function saveSmartAlbum(smartAlbum: any) {
  const albums = [...(libConfig.smartAlbums || [])];
  const index = albums.findIndex((item: any) => item.id === smartAlbum.id);
  if (index >= 0) {
    albums[index] = smartAlbum;
  } else {
    albums.push(smartAlbum);
  }
  libConfig.smartAlbums = albums;
  libConfig.smartAlbum.type = 'custom';
  libConfig.smartAlbum.id = smartAlbum.id;
  closeEditDialog();
  if (isSmartAlbumHidden(smartAlbum.id)) {
    await libConfig.save();
    await tauriEmit('library-total-refreshed', { source: 'hidden-filters' });
  }
}

function clickDeleteSmartAlbum(smartAlbum: any) {
  deletingSmartAlbum.value = smartAlbum;
  showDeleteDialog.value = true;
}

function closeDeleteDialog() {
  showDeleteDialog.value = false;
  deletingSmartAlbum.value = null;
}

async function confirmDeleteSmartAlbum() {
  const smartAlbum = deletingSmartAlbum.value;
  if (!smartAlbum) return;
  const wasHidden = isSmartAlbumHidden(smartAlbum.id);
  libConfig.smartAlbums = (libConfig.smartAlbums || []).filter((item: any) => item.id !== smartAlbum.id);
  if (wasHidden) {
    await libConfig.removeHiddenSmartAlbum(String(smartAlbum.id));
    await tauriEmit('library-total-refreshed', { source: 'hidden-filters' });
  }
  if (libConfig.smartAlbum.type === 'custom' && libConfig.smartAlbum.id === smartAlbum.id) {
    libConfig.smartAlbum.type = 'system';
    libConfig.smartAlbum.id = null;
  }
  closeDeleteDialog();
}
</script>
