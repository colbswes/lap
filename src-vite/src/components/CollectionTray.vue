<template>
  <section
    data-collection-tray-root="true"
    :data-collection-drop-new="collections.length === 0 ? 'true' : undefined"
    class="collection-tray min-h-0 flex flex-col  border-t border-base-content/5 shadow-sm"
    :class="libConfig.activePane === 'collection' ? '' : 'sidebar-pane-inactive'"
  >
    <div class="sidebar-panel-header cursor-pointer" @click="$emit('toggle-expanded')">
      <TButton
        :icon="IconRight"
        :buttonSize="'small'"
        iconClasses="transition-transform duration-200"
        :iconStyle="{ transform: expanded ? 'rotate(90deg)' : 'rotate(0deg)' }"
        @click.stop="$emit('toggle-expanded')"
      />
      <span class="sidebar-panel-header-title flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">
        {{ $t('collection.title') }}<template v-if="collections.length > 0"> ({{ collections.length.toLocaleString() }})</template>
      </span>
      <span v-if="isItemDragging" class="badge badge-sm badge-primary badge-outline shrink-0">
        {{ $t('collection.drop_title_hint') }}
      </span>
      <TButton
        v-if="expanded"
        :icon="IconAdd"
        :buttonSize="'small'"
        :tooltip="$t('collection.add')"
        :disabled="collections.length >= maxCollectionCount"
        @click.stop="addCollection"
      />
      <ContextMenu
        v-if="expanded"
        data-collection-reorder-area
        :menuItems="collectionPanelMenuItems"
        :iconMenu="IconMore"
        :smallIcon="true"
        @click.stop
        @open-change="collectionPanelMenuOpen = $event"
      />
    </div>

    <transition
      enter-active-class="transition-all duration-200 ease-out"
      enter-from-class="opacity-0 -translate-y-1"
      enter-to-class="opacity-100 translate-y-0"
      leave-active-class="transition-all duration-150 ease-in"
      leave-from-class="opacity-100 translate-y-0"
      leave-to-class="opacity-0 -translate-y-1"
    >
      <div v-if="expanded" class="flex min-h-0 flex-1 transform-gpu overflow-hidden">
        <div class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto pb-1">
          <div v-if="collections.length > 10" class="mx-1 mb-2 px-1 shrink-0">
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
                :placeholder="$t('collection.search')"
                class="w-full min-w-0 bg-transparent border-none focus:ring-0 px-2 text-sm placeholder-base-content/30 focus:outline-none"
                @focus="isSearchFocused = true"
                @blur="isSearchFocused = false"
              />
              <button
                v-if="searchQuery"
                type="button"
                class="mr-1 p-1 rounded-box text-base-content/30 hover:text-base-content/70"
                @click="searchQuery = ''"
              >
                <IconClose class="w-4 h-4" />
              </button>
            </div>
          </div>
          <VueDraggable
            v-model="collections"
            class="px-1"
            :animation="150"
            handle=".collection-drag-handle"
            direction="vertical"
            :group="{ name: 'collection-tray', pull: false, put: false }"
            :force-fallback="true"
            ghost-class="sidebar-reorder-placeholder"
            :revertOnSpill="true"
            :bubble-scroll="false"
            :disabled="!isReorderingCollections || !canReorderCollections"
            @start="onReorderStart"
            @end="onReorderEnd"
            @drop.stop
          >
            <div
              v-for="collection in filteredCollections"
              :key="collection.id"
              data-collection-reorder-area
              :data-collection-drop-id="renamingId === collection.id ? undefined : collection.id"
              :class="[
                'sidebar-item group border-2 border-transparent transition-colors',
                selectedId === collection.id ? 'sidebar-item-selected' : 'sidebar-item-hover',
                isCollectionHidden(collection.id) ? 'opacity-50' : '',
              ]"
              @click="selectCollection(collection)"
            >
            <IconDragHandle
              v-if="isReorderingCollections && canReorderCollections"
              class="collection-drag-handle p-1 w-6 h-6 shrink-0 cursor-grab text-base-content/30"
              :title="$t('collection.reorder')"
              @click.stop
            />
            <component :is="collectionKindIcon(collection.kind)" class="pr-1 w-6 h-6 shrink-0" />
            <input
              v-if="renamingId === collection.id"
              ref="renameInputRef"
              v-model="renameValue"
              class="input px-1 min-w-0 flex-1 text-base"
              maxlength="64"
              @click.stop
              @mousedown.stop
              @keydown.enter.prevent="commitRename(collection)"
              @keydown.escape.prevent="cancelRename"
              @blur="commitRename(collection)"
            />
            <span v-else class="sidebar-item-label">{{ collection.name }}</span>
            <div class="ml-auto flex flex-row items-center text-base-content/30">
              <span
                v-if="renamingId !== collection.id && getCollectionDisplayCount(collection) > 0"
                class="sidebar-item-count shrink-0"
              >
                {{ getCollectionDisplayCount(collection).toLocaleString() }}
              </span>
              <button
                v-if="renamingId !== collection.id"
                type="button"
                :title="isCollectionHidden(collection.id) ? $t('collection.show') : $t('collection.hide')"
                :aria-label="isCollectionHidden(collection.id) ? $t('collection.show') : $t('collection.hide')"
                :aria-pressed="isCollectionHidden(collection.id)"
                :class="[
                  'p-1 rounded-box text-base-content/45 hover:text-base-content hover:bg-base-100/40 cursor-pointer shrink-0 transition-colors',
                  isCollectionHidden(collection.id) ? 'flex' : 'hidden group-hover:flex',
                ]"
                @click.stop="toggleCollectionHidden(collection)"
              >
                <component :is="isCollectionHidden(collection.id) ? IconHide : IconUnhide" class="w-4 h-4" />
              </button>
              <div
                v-if="renamingId !== collection.id"
                :class="[
                  selectedId === collection.id ? '' : 'hidden group-hover:block',
                ]"
              >
                <ContextMenu
                  :iconMenu="IconMore"
                  :menuItems="collectionMenuItems(collection)"
                  :smallIcon="true"
                />
              </div>
            </div>
            </div>
          </VueDraggable>
          <div v-if="collections.length > 0 && filteredCollections.length === 0" class="sidebar-empty text-sm">
            <span class="text-center">{{ $t('collection.not_found') }}</span>
          </div>
          <div
            v-if="!isLoadingCollections && collections.length === 0 && !renamingId"
            class="mt-2 px-3 py-3 flex flex-col items-center gap-1 text-center text-base-content/30"
          >
            <span class="text-sm">{{ $t('collection.empty_content') }}</span>
            <span class="text-xs">{{ $t('collection.drop_here') }}</span>
          </div>
        </div>
      </div>
    </transition>

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
    <MessageBox
      v-if="clearTarget"
      :title="$t('collection.clear_confirm_title')"
      :message="$t('collection.clear_confirm_message', { name: clearTarget.name })"
      :OkText="$t('collection.clear_confirm_ok')"
      :cancelText="$t('msgbox.cancel')"
      @ok="confirmClear"
      @cancel="clearTarget = null"
    />
    <TaggingDialog
      v-if="taggingFileIds.length > 0"
      :fileIds="taggingFileIds"
      :file-name="taggingCollectionName"
      position-key="edit-tag"
      @ok="closeTaggingDialog"
      @cancel="closeTaggingDialog"
    />
  </section>
</template>

<script setup lang="ts">
import { computed, nextTick, onBeforeUnmount, onMounted, ref, watch } from 'vue';
import { emit as tauriEmit, listen } from '@tauri-apps/api/event';
import { useI18n } from 'vue-i18n';
import { useUIStore } from '@/stores/uiStore';
import { config, libConfig } from '@/common/config';
import { clearCollection, createCollection, deleteCollection as deleteCollectionApi, getCollectionCounts, getCollectionQueryFileIds, listCollections, renameCollection, reorderCollections, setCollectionKind } from '@/common/api';
import { collectionKindIcon, COLLECTION_KINDS, type CollectionKind } from '@/common/collectionKinds';
import { isHiddenId } from '@/common/hiddenAlbums';
import { SIDEBAR } from '@/common/constants';
import { buildSmartDraftFromCollection, makeDefaultQueryParams } from '@/common/collectionActions';
import { useLibraryChangeRefresh } from '@/composables/useLibraryChangeRefresh';
import { IconAdd, IconRight, IconRename, IconMore, IconFolderCog, IconHide, IconTag, IconTrash, IconClose, IconSearch, IconDragHandle, IconOrder, IconUnhide, IconOk } from '@/common/icons';
import { VueDraggable, type DraggableEvent } from 'vue-draggable-plus';
import ContextMenu from '@/components/ContextMenu.vue';
import MessageBox from '@/components/MessageBox.vue';
import TaggingDialog from '@/components/TaggingDialog.vue';
import TButton from '@/components/TButton.vue';

const props = defineProps({
  expanded: {
    type: Boolean,
    required: true,
  },
});

const emit = defineEmits(['toggle-expanded']);

const { t } = useI18n();
const uiStore = useUIStore();
type Collection = {
  id: number;
  name: string;
  sortOrder?: number;
  count: number;
  kind: CollectionKind;
};

const collections = ref<Collection[]>([]);
const isLoadingCollections = ref(true);
const maxCollectionCount = computed(() => Math.max(1, Number(config.main.maxCollectionCount) || 100));
const searchQuery = ref('');
const isSearchFocused = ref(false);
const filteredCollections = computed(() => {
  const query = searchQuery.value.trim().toLocaleLowerCase();
  return query
    ? collections.value.filter(collection => collection.name.toLocaleLowerCase().includes(query))
    : collections.value;
});
watch(() => collections.value.length, (count) => {
  if (count <= 10) searchQuery.value = '';
});
watch(() => libConfig.activePane, () => {
  if (libConfig.activePane === 'collection') void loadCollections();
});
const selectedId = ref<number | null>(Number(libConfig.collection.selectedId || 0) || null);
const getCollectionDisplayCount = (collection: Collection) => Number(libConfig.collection.counts?.[String(collection.id)] || 0);
const isReorderingCollections = ref(false);
const collectionPanelMenuOpen = ref(false);
const renamingId = ref<number | null>(null);
const renameValue = ref('');
const renameInputRef = ref<HTMLInputElement | HTMLInputElement[] | null>(null);
const isItemDragging = ref(false);
const isCollectionHidden = (collectionId: number) =>
  isHiddenId(libConfig.hiddenCollectionIds, Number(collectionId));

const hiddenCollectionCount = computed(() =>
  collections.value.filter(collection => isCollectionHidden(collection.id)).length
);

async function toggleCollectionHidden(collection: Collection) {
  await libConfig.toggleCollectionHidden(Number(collection.id));
  await tauriEmit('library-total-refreshed', { source: 'hidden-filters' });
}

async function showAllCollections() {
  await libConfig.showAllCollections();
  await tauriEmit('library-total-refreshed', { source: 'hidden-filters' });
}

const canReorderCollections = computed(() =>
  collections.value.length > 1 && !searchQuery.value.trim() && renamingId.value === null && !isItemDragging.value
);
const collectionPanelMenuItems = computed(() => [
  {
    label: t('collection.reorder'),
    icon: IconOrder,
    disabled: !canReorderCollections.value,
    action: () => { isReorderingCollections.value = !isReorderingCollections.value; },
  },
  ...(hiddenCollectionCount.value > 0 ? [
    {
      label: t('collection.show_all'),
      icon: IconUnhide,
      action: () => { void showAllCollections(); },
    },
  ] : []),
]);
watch(canReorderCollections, (canReorder) => {
  if (!canReorder) isReorderingCollections.value = false;
});
watch(() => props.expanded, (expanded) => {
  if (expanded) return;
  isReorderingCollections.value = false;
  collectionPanelMenuOpen.value = false;
});
const deleteTarget = ref<Collection | null>(null);
const clearTarget = ref<Collection | null>(null);
const taggingFileIds = ref<number[]>([]);
const taggingCollectionName = ref('');
let unlistenCollectionFilesDropped: (() => void) | null = null;
let unlistenCollectionsChanged: (() => void) | null = null;
let unlistenContentItemsDragState: (() => void) | null = null;
let unlistenLibrarySwitched: (() => void) | null = null;
let collectionCountRequest = 0;
let collectionListRequest = 0;
let isCollectionTrayMounted = true;

async function refreshCollectionCounts() {
  const request = ++collectionCountRequest;
  const libraryId = libConfig._libraryId;
  const counts = await getCollectionCounts();
  if (isCollectionTrayMounted && request === collectionCountRequest && counts && libraryId === libConfig._libraryId) {
    libConfig.collection.counts = counts;
  }
}
useLibraryChangeRefresh(refreshCollectionCounts);

onMounted(async () => {
  document.addEventListener('pointerdown', handleReorderOutsidePointerDown, true);
  try {
    await loadCollections();
  } finally {
    isLoadingCollections.value = false;
  }
  unlistenCollectionFilesDropped = await listen('collection-files-dropped', async () => {
    await loadCollections();
  });
  unlistenCollectionsChanged = await listen('collections-changed', async (event: any) => {
    const preferredId = Number(event?.payload?.selectedId || 0) || undefined;
    await loadCollections(preferredId);
  });
  unlistenContentItemsDragState = await listen('content-items-drag-state', (event: any) => {
    isItemDragging.value = Boolean(event.payload?.dragging);
  });
  unlistenLibrarySwitched = await listen('library-switched', async () => {
    selectedId.value = Number(libConfig.collection.selectedId || 0) || null;
    isReorderingCollections.value = false;
    renamingId.value = null;
    renameValue.value = '';
    deleteTarget.value = null;
    await loadCollections();
  });
});

onBeforeUnmount(() => {
  isCollectionTrayMounted = false;
  collectionCountRequest++;
  document.removeEventListener('pointerdown', handleReorderOutsidePointerDown, true);
  uiStore.removeInputHandler('CollectionTrayDrag');
  unlistenCollectionFilesDropped?.();
  unlistenCollectionFilesDropped = null;
  unlistenCollectionsChanged?.();
  unlistenCollectionsChanged = null;
  unlistenContentItemsDragState?.();
  unlistenContentItemsDragState = null;
  unlistenLibrarySwitched?.();
  unlistenLibrarySwitched = null;
});

watch(() => config.settings.showEvents, () => { void loadCollections(); });

async function loadCollections(preferredId?: number) {
  const request = ++collectionListRequest;
  const libraryId = libConfig._libraryId;
  const [result] = await Promise.all([
    listCollections(),
    refreshCollectionCounts(),
  ]);
  if (
    !isCollectionTrayMounted
    || request !== collectionListRequest
    || libraryId !== libConfig._libraryId
  ) return;
  const countMap = libConfig.collection.counts || {};
  // Keep event collections in the tray when the Events sidebar is hidden.
  collections.value = Array.isArray(result)
    ? result.filter((item: any) => !config.settings.showEvents || !item.kind).map((item: any) => ({
      id: Number(item.id),
      name: String(item.name || ''),
      sortOrder: Number(item.sortOrder || 0),
      count: Number(countMap[String(item.id)] || 0),
      kind: (item.kind || null) as CollectionKind,
    }))
    : [];

  const nextSelectedId = Number(preferredId || libConfig.collection.selectedId || selectedId.value || 0);
  const selected = collections.value.find(item => item.id === nextSelectedId) || null;
  if (selected) {
    selectedId.value = selected.id;
    libConfig.collection.selectedId = selected.id;
  } else {
    selectedId.value = null;
    libConfig.collection.selectedId = null;
  }
}

function selectCollection(collection: Collection) {
  libConfig.activePane = 'collection';
  selectedId.value = collection.id;
  libConfig.collection.selectedId = collection.id;
  uiStore.requestCountUpdate({ source: 'collection', id: Number(collection.id) });
  libConfig.collection.activateTick = Number(libConfig.collection.activateTick || 0) + 1;
}

async function addCollection() {
  if (collections.value.length >= maxCollectionCount.value) return;
  const collection = await createCollection(t('collection.default_name', { index: collections.value.length + 1 }));
  if (!collection?.id) return;
  await loadCollections(Number(collection.id));
  const created = collections.value.find(item => item.id === Number(collection.id));
  if (created) {
    selectCollection(created);
    startRename(created);
  }
}

async function startRename(collection: Collection) {
  selectCollection(collection);
  renamingId.value = collection.id;
  renameValue.value = collection.name;
  await nextTick();
  const input = Array.isArray(renameInputRef.value)
    ? renameInputRef.value[0]
    : renameInputRef.value;
  input?.focus({ preventScroll: true });
  input?.select();
}

async function commitRename(collection: Collection) {
  if (renamingId.value !== collection.id) return;
  const nextName = renameValue.value.trim();
  if (nextName && nextName !== collection.name) {
    await renameCollection(collection.id, nextName);
    await loadCollections(collection.id);
    await tauriEmit('refresh-content');
  }
  cancelRename();
}

function cancelRename() {
  renamingId.value = null;
  renameValue.value = '';
}

function deleteCollection(collection: Collection) {
  deleteTarget.value = collection;
}

async function confirmDelete() {
  const target = deleteTarget.value;
  if (!target) return;
  const wasSelected = Number(libConfig.collection.selectedId || 0) === target.id;
  const targetIndex = collections.value.findIndex(item => item.id === target.id);
  const replacementId = wasSelected && targetIndex >= 0
    ? collections.value[targetIndex + 1]?.id || collections.value[targetIndex - 1]?.id
    : undefined;
  const wasHidden = isCollectionHidden(target.id);
  deleteTarget.value = null;
  await deleteCollectionApi(target.id);
  if (wasHidden) {
    await libConfig.removeHiddenCollection(Number(target.id));
    await tauriEmit('library-total-refreshed', { source: 'hidden-filters' });
  }
  await loadCollections(replacementId);
  await tauriEmit('refresh-content');
}

async function confirmClear() {
  const target = clearTarget.value;
  if (!target) return;
  clearTarget.value = null;
  await clearCollection(target.id);
  await loadCollections(Number(libConfig.collection.selectedId || 0));
  await tauriEmit('refresh-content');
}

async function openTaggingForCollection(collection: Collection) {
  const fileIds = await getCollectionQueryFileIds(collection.id, makeDefaultQueryParams());
  if (!Array.isArray(fileIds) || fileIds.length === 0) return;
  taggingCollectionName.value = collection.name;
  taggingFileIds.value = fileIds.map((id: any) => Number(id)).filter((id: number) => id > 0);
}

function closeTaggingDialog() {
  taggingFileIds.value = [];
  taggingCollectionName.value = '';
}

function createSmartFromCollection(collection: Collection) {
  void tauriEmit('create-smart-album-from-draft', buildSmartDraftFromCollection(collection));
}

function collectionMenuItems(collection: Collection) {
  const hidden = isCollectionHidden(collection.id);
  return [
    {
      label: t('collection.rename'),
      icon: IconRename,
      action: () => startRename(collection),
    },
    {
      label: hidden ? t('collection.show') : t('collection.hide'),
      icon: hidden ? IconUnhide : IconHide,
      action: () => { void toggleCollectionHidden(collection); },
    },
    {
      label: t('collection.type'),
      icon: collectionKindIcon(collection.kind),
      children: COLLECTION_KINDS.map(kind => ({
        label: t(`collection.type_${kind || 'none'}`),
        icon: collection.kind === kind ? IconOk : undefined,
        action: () => { void changeCollectionKind(collection, kind); },
      })),
    },
    { label: "-", action: null },
    {
      label: t('collection.create_smart_collection'),
      icon: IconFolderCog,
      action: () => createSmartFromCollection(collection),
    },
    {
      label: t('collection.tag_photos'),
      icon: IconTag,
      disabled: getCollectionDisplayCount(collection) === 0,
      action: () => { void openTaggingForCollection(collection); },
    },
    {
      label: t('collection.clear'),
      action: () => { clearTarget.value = collection; },
    },
    { label: "-", action: null },
    {
      label: t('collection.delete'),
      icon: IconTrash,
      action: () => deleteCollection(collection),
    },
  ];
}

async function changeCollectionKind(collection: Collection, kind: CollectionKind) {
  if (collection.kind === kind) return;
  if (await setCollectionKind(collection.id, kind) == null) return;
  collection.kind = kind;
  await tauriEmit('collections-changed', { selectedId: selectedId.value });
  if (kind && config.settings.showEvents) {
    // It is an event now: show it in the Events sidebar.
    Object.assign(libConfig.event, { selectedId: collection.id, artistId: 0, artistName: '' });
    libConfig.activePane = 'main';
    config.main.sidebarIndex = SIDEBAR.EVENT;
  }
}

function handleReorderOutsidePointerDown(event: PointerEvent) {
  if (event.button !== 0 || !isReorderingCollections.value || collectionPanelMenuOpen.value) return;
  if (event.target instanceof Element && event.target.closest('[data-collection-reorder-area]')) return;
  isReorderingCollections.value = false;
}

function onReorderStart() {
  uiStore.removeInputHandler('CollectionTrayDrag');
  uiStore.pushInputHandler('CollectionTrayDrag');
}

async function onReorderEnd({ oldIndex, newIndex }: DraggableEvent) {
  setTimeout(() => uiStore.removeInputHandler('CollectionTrayDrag'), 0);
  if (oldIndex === newIndex) return;
  try {
    await reorderCollections(collections.value.map((collection, sortOrder) => ({ id: collection.id, sortOrder })));
  } catch (error) {
    console.error('Failed to reorder collections:', error);
    await loadCollections();
  }
}
</script>
