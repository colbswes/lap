<template>
  <div class="sidebar-panel" @pointerdown.capture="activateTagPanel" @focusin="activateTagPanel">
    <div class="sidebar-panel-header">
      <span class="sidebar-panel-header-title flex-1"
        >{{ $t("tag.title")
        }}<template v-if="tags.length">
          ({{ tags.length.toLocaleString() }})</template
        ></span
      >
      <TButton
      :icon="IconTagAdd"
      buttonSize="small"
      :tooltip="$t('msgbox.new_tag.title')"
      @click="clickAddTag()"
      />
      <div class="inline-flex shrink-0 items-center gap-0 overflow-hidden rounded-box"
        :class="{ 'border border-base-content/5': libConfig.tag.multi }">
        <TButton :icon="IconMultiSelect" buttonSize="small" :selected="libConfig.tag.multi"
          :buttonClasses="libConfig.tag.multi ? 'h-6! w-6! min-h-0!' : ''"
          :disabled="!tags.length" :tooltip="$t(libConfig.tag.multi ? 'tag.exit_multi_filter' : 'tag.multi_filter')"
          @click="libConfig.tag.multi ? exitMultiFilter() : enterTagFilter(libConfig.tag)" />
        <Transition name="tag-filter-modes">
          <div v-if="libConfig.tag.multi" class="tag-filter-modes-slot shrink-0 overflow-hidden">
            <div class="flex w-16 items-center">
              <button v-for="any in [false, true]" :key="String(any)"
                class="h-6 w-8 shrink-0 rounded-box text-[10px] cursor-pointer hover:bg-base-100/30"
                :class="libConfig.tag.matchAny === any ? 'text-primary bg-primary/15' : 'text-base-content/50'"
                :aria-pressed="libConfig.tag.matchAny === any"
                @click="libConfig.tag.matchAny = any">{{ any ? 'OR' : 'AND' }}</button>
            </div>
          </div>
        </Transition>
      </div>
      <TButton
        v-if="hiddenTagCount > 0"
        :icon="IconUnhide"
        buttonSize="small"
        :tooltip="$t('tag.show_all')"
        @click="showAllTags"
      />
      <SortMenuButton v-model="config.settings.tagSort" kind="category" />
    </div>
    <div
      v-if="libConfig.tag.multi && selectedTagObjects.length > 0"
      class="mx-1 mb-1.5 px-1.5 py-1 rounded-box bg-primary/10 border border-primary/20 flex items-center justify-between gap-1 text-[11px]"
    >
      <button
        type="button"
        class="flex items-center gap-1 text-primary hover:underline cursor-pointer truncate"
        @click="createSmartFromSelectedTags"
      >
        <IconFolderCog class="w-3.5 h-3.5 shrink-0" />
        <span class="truncate">{{ $t("collection.create_smart_collection") }}</span>
      </button>
      <button
        type="button"
        class="text-base-content/65 hover:text-primary cursor-pointer shrink-0"
        @click="saveSelectedTagsAsCollection"
      >
        {{ $t("collection.save_as_collection") }}
      </button>
    </div>
    <div v-if="tags.length > 0" class="mx-1 mb-2 px-1 shrink-0">
      <div
        :class="[
          'h-8 flex items-center rounded-box transition-colors bg-base-100/40',
          isSearchFocused
            ? 'border-2 border-primary'
            : 'border border-base-content/10 hover:border-base-content/30',
        ]"
        @click="searchInputRef?.focus()"
      >
        <IconSearch
          class="ml-2 w-4 h-4 shrink-0"
          :class="isSearchFocused ? 'text-primary/70' : 'text-base-content/30'"
        />
        <input
          ref="searchInputRef"
          type="text"
          v-model="search"
          :placeholder="$t('menu.tag.search')"
          class="w-full min-w-0 bg-transparent border-none focus:ring-0 px-2 text-sm placeholder-base-content/30 focus:outline-none"
          maxlength="255"
          @focus="isSearchFocused = true"
          @blur="isSearchFocused = false"
        />
        <button
          v-if="search"
          type="button"
          :title="$t('menu.tag.clear_search')"
          class="mr-1 p-1 rounded-box text-base-content/30 hover:text-base-content/70"
          @click.stop="
            search = '';
            searchInputRef?.focus();
          "
        >
          <IconClose class="w-4 h-4" />
        </button>
      </div>
    </div>

    <div ref="tagList" class="grow min-h-0 overflow-y-auto overflow-x-hidden">
      <ul role="tree" :aria-multiselectable="!!libConfig.tag.multi">
        <li
          v-for="tag in rows"
          :key="tag.id"
          role="treeitem"
          :aria-level="tag.depth + 1"
          :aria-expanded="tag.hasChildren ? expanded(tag.id) : undefined"
          :aria-selected="isTagSelected(tag.id)"
        >
          <div
            :id="`tag-${tag.id}`"
            class="sidebar-item group select-none focus:outline-none"
            :class="[
              isTagSelected(tag.id) ? 'sidebar-item-selected' : 'sidebar-item-hover',
              {
                'opacity-50': dragged === tag.id || isTagHidden(tag.id),
                'bg-primary/10 ring-1 ring-inset ring-primary/40':
                  dropTarget === tag.id,
              },
            ]"
            :style="{ paddingLeft: `${4 + tag.depth * 16}px` }"
            :data-tag-drop-id="tag.id"
            tabindex="0"
            @keydown.enter.stop.prevent="selectTag(tag)"
            @keydown.space.stop.prevent="selectTag(tag)"
            @pointerdown="startDrag($event, tag)"
            @dragstart.prevent
            @click="selectTag(tag, $event)"
            @dblclick.stop="!libConfig.tag.multi && edit(tag)"
          >
            <button
              v-if="tag.hasChildren"
              class="size-6 shrink-0 flex items-center justify-center"
              :aria-label="tag.name"
              @click.stop="toggle(tag.id)"
            >
              <IconRight
                class="size-6 p-1 transition-transform"
                :class="{ 'rotate-90': expanded(tag.id) }"
              />
            </button>
            <span v-else class="size-6 shrink-0" aria-hidden="true" />
            <input v-if="libConfig.tag.multi" type="checkbox" class="checkbox checkbox-xs mr-1 shrink-0"
              :class="{ 'checkbox-primary': isTagSelected(tag.id) }"
              :checked="isTagSelected(tag.id)" :aria-label="tag.path || tag.name"
              @click.stop="selectTag(tag)" />
            <IconTag class="w-5 h-5 pr-1 shrink-0" />
            <TagNameInput
              v-if="editor && tag.id === (editor.id ?? -1)"
              :key="tag.id"
              :initial-name="editor.name"
              :saving="saving"
              :error="error"
              @save="saveEditor"
              @cancel="cancelEditor"
              @clear-error="error = ''"
            />
            <span v-else class="sidebar-item-label">{{
              search ? tag.path : tag.name
            }}</span>
            <div class="ml-auto flex flex-row items-center text-base-content/30">
              <span v-if="Number(tag.count) > 0" class="sidebar-item-count shrink-0">{{
                Number(tag.count).toLocaleString()
              }}</span>
              <button
                v-if="!editor && tag.id > 0"
                type="button"
                :title="isTagHidden(tag.id) ? $t('tag.show') : $t('tag.hide')"
                :aria-label="isTagHidden(tag.id) ? $t('tag.show') : $t('tag.hide')"
                :aria-pressed="isTagHidden(tag.id)"
                :class="[
                  'p-1 rounded-box text-base-content/45 hover:text-base-content hover:bg-base-100/40 cursor-pointer shrink-0 transition-colors',
                  isTagHidden(tag.id) ? 'flex' : 'hidden group-hover:flex',
                ]"
                @click.stop="toggleTagHidden(tag)"
              >
                <component :is="isTagHidden(tag.id) ? IconHide : IconUnhide" class="w-4 h-4" />
              </button>
              <div
                v-if="!editor"
                class="hidden group-hover:block"
                @click.stop
              >
                <ContextMenu
                  :iconMenu="IconMore"
                  :menuItems="() => tagMenu(tag)"
                  :smallIcon="true"
                />
              </div>
            </div>
          </div>
        </li>
      </ul>
      <div
        v-if="dragged !== null"
        data-tag-drop-root
        class="sticky bottom-0 sidebar-item border border-dashed border-primary/40 bg-base-200 text-primary"
        :class="{ 'ring-1 ring-inset ring-primary': dropTarget === null }"
      >
        {{ $t("tag.root") }}
      </div>
      <div v-if="!loading && !rows.length" class="sidebar-empty text-sm">
        {{ $t(failed ? "tag.load_failed" : "tag.not_found") }}
      </div>
    </div>
  </div>
  <MessageBox
    v-if="deleteTarget"
    :title="$t('msgbox.delete_tag.title', { tag: deleteTarget.name })"
    :message="$t('tag.delete_tree')"
    :warningOk="true"
    :isLoading="saving"
    :OkText="$t('msgbox.delete_tag.ok')"
    :cancelText="$t('msgbox.cancel')"
    @ok="confirmDelete"
    @cancel="deleteTarget = null"
  />
</template>

<script setup lang="ts">
import {
  ref,
  computed,
  watch,
  nextTick,
  onMounted,
  onBeforeUnmount,
} from "vue";
import { emit as emitEvent, listen } from "@tauri-apps/api/event";
import { useI18n } from "vue-i18n";
import { useToast } from "@/common/toast";
import { useUIStore } from "@/stores/uiStore";
import { selectFilterTag, exitTagFilter, enterTagFilter } from '@/common/tagFilter';
import { useLibraryChangeRefresh } from "@/composables/useLibraryChangeRefresh";
import { config, libConfig } from "@/common/config";
import { SIDEBAR } from "@/common/constants";
import {
  getAllTags,
  getTagCounts,
  getQueryFileIds,
  createTag,
  renameTag,
  deleteTag,
  moveTag,
  getTagSubtreeInfo,
} from "@/common/api";
import { isHiddenId } from "@/common/hiddenAlbums";
import {
  buildSmartDraftFromTags,
  makeDefaultQueryParams,
  saveFilesAsNewCollection,
} from "@/common/collectionActions";
import {
  tagRows,
  ancestorIds,
  subtreeIds,
  isValidTagName,
  type LibraryTag,
} from "@/common/tagTree";
import {
  IconClose,
  IconCollection,
  IconFolderCog,
  IconHide,
  IconMultiSelect,
  IconMore,
  IconSearch,
  IconTag,
  IconTagAdd,
  IconRight,
  IconRename,
  IconTrash,
  IconUnhide,
} from "@/common/icons";
import ContextMenu from "./ContextMenu.vue";
import TButton from "./TButton.vue";
import MessageBox from "./MessageBox.vue";
import TagNameInput from "./TagNameInput.vue";
import SortMenuButton from "./SortMenuButton.vue";

defineProps<{ titlebar: string }>();
defineEmits(["editDataChanged"]);
const { t } = useI18n();
const toast = useToast();
const uiStore = useUIStore();
const tagsChangeSource = crypto.randomUUID();
const tags = ref<LibraryTag[]>([]);
const search = ref("");
const searchInputRef = ref<HTMLInputElement | null>(null);
const isSearchFocused = ref(false);
const loading = ref(true);
const failed = ref(false);
const saving = ref(false);
const error = ref("");
const editor = ref<{
  id: number | null;
  name: string;
  parent: number | null;
} | null>(null);
const deleteTarget = ref<
  (LibraryTag & { tag_count: number; file_count: number }) | null
>(null);
const dragged = ref<number | null>(null);
const tagList = ref<HTMLElement | null>(null);
const dropTarget = ref<number | null | undefined>(undefined);
const rows = computed(() =>
  tagRows(
    editor.value?.id === null
      ? [
          ...tags.value,
          {
            id: -1,
            name: editor.value.name,
            parent_id: editor.value.parent,
            path: editor.value.name,
          },
        ]
      : tags.value,
    search.value,
    libConfig.tag.collapsedTagIds || [],
    config.settings.tagSort,
    -1,
  ),
);
const expanded = (id: number) =>
  !!search.value || !(libConfig.tag.collapsedTagIds || []).includes(id);
function toggle(id: number) {
  if (search.value) return;
  const closed = libConfig.tag.collapsedTagIds || [];
  libConfig.tag.collapsedTagIds = closed.includes(id)
    ? closed.filter((value: number) => value !== id)
    : [...closed, id];
}
function activateTagPanel() {
  uiStore.setActivePane('left-sidebar');
  libConfig.activePane = 'main';
}
function isTagSelected(id: number) {
  return libConfig.tag.multi ? (libConfig.tag.selectedIds || []).includes(id) : libConfig.tag.id === id;
}
async function exitMultiFilter() {
  exitTagFilter(libConfig.tag);
  const id = libConfig.tag.id;
  if (id == null) return;
  search.value = '';
  reveal(id);
  libConfig.tag.activateTick++;
  await nextTick();
  if (libConfig.tag.multi || libConfig.tag.id !== id) return;
  const row = document.getElementById(`tag-${id}`);
  row?.scrollIntoView({ block: 'nearest' });
}
async function handleFilterEscape(event: KeyboardEvent) {
  if (event.key !== 'Escape' || event.isComposing) return;
  const canHandle = () => !disposed && !event.defaultPrevented && libConfig.tag.multi && !editor.value
    && uiStore.activePane === 'left-sidebar' && config.main.sidebarIndex === SIDEBAR.TAG && libConfig.activePane === 'main'
    && !uiStore.inputStack.length && !document.fullscreenElement && !document.querySelector('[role="dialog"]');
  if (!canHandle()) return;
  // A new task lets preview/fullscreen handlers consume Escape before we recheck.
  await new Promise<void>(resolve => setTimeout(resolve, 0));
  if (!canHandle()) return;
  if (search.value && searchInputRef.value === document.activeElement) search.value = '';
  else exitMultiFilter();
}
function selectTag(tag: LibraryTag, event?: MouseEvent) {
  if (suppressDragClick) return;
  selectFilterTag(libConfig.tag, tag.id, !!(event?.metaKey || event?.ctrlKey));
}
function reveal(id: number) {
  const ancestors = new Set(ancestorIds(tags.value, id));
  libConfig.tag.collapsedTagIds = (libConfig.tag.collapsedTagIds || []).filter(
    (value: number) => !ancestors.has(value),
  );
}
function cancelEditor() {
  if (!saving.value) editor.value = null;
}
function clickAddTag(parent: number | null = null) {
  if (saving.value || editor.value) return;
  error.value = "";
  search.value = "";
  const base = t("msgbox.new_tag.title");
  let name = base;
  let index = 0;
  const names = new Set(
    tags.value
      .filter((tag) => tag.parent_id === parent)
      .map((tag) => tag.name.toLocaleLowerCase()),
  );
  while (names.has(name.toLocaleLowerCase())) name = `${base} ${++index}`;
  editor.value = { id: null, name, parent };
  if (parent !== null) {
    const open = new Set([parent, ...ancestorIds(tags.value, parent)]);
    libConfig.tag.collapsedTagIds = (
      libConfig.tag.collapsedTagIds || []
    ).filter((id: number) => !open.has(id));
  }
}
function edit(tag: LibraryTag) {
  if (saving.value || editor.value) return;
  error.value = "";
  search.value = "";
  editor.value = { id: tag.id, name: tag.name, parent: tag.parent_id };
  reveal(tag.id);
}
async function saveEditor(value: string) {
  const target = editor.value;
  if (!target || saving.value) return;
  const name = value.trim();
  if (!isValidTagName(name)) {
    error.value = t("tag.invalid_name");
    return;
  }
  if (
    target.id !== null &&
    tags.value.find((tag) => tag.id === target.id)?.name === name
  ) {
    editor.value = null;
    return;
  }
  if (
    tags.value.some(
      (tag) =>
        tag.id !== target.id &&
        tag.parent_id === target.parent &&
        tag.name.toLocaleLowerCase() === name.toLocaleLowerCase(),
    )
  ) {
    error.value = t("tag.name_exists");
    return;
  }
  saving.value = true;
  try {
    const result =
      target.id === null
        ? await createTag(name, target.parent)
        : await renameTag(target.id, name);
    if (!result) throw new Error("save failed");
    editor.value = null;
    search.value = "";
    await load();
    const id = target.id ?? result.id;
    reveal(id);
    libConfig.tag.id = id;
    await notifyChanged();
    await nextTick();
    document.getElementById(`tag-${id}`)?.scrollIntoView({ block: "nearest" });
  } catch {
    error.value = t("tag.name_save_failed");
  } finally {
    saving.value = false;
  }
}
const isTagHidden = (tagId: number) =>
  isHiddenId(libConfig.hiddenTagIds, Number(tagId));

const hiddenTagCount = computed(() =>
  tags.value.filter((tag) => isTagHidden(tag.id)).length,
);

const selectedTagObjects = computed(() => {
  const selected = new Set((libConfig.tag.selectedIds || []).map((id: any) => Number(id)));
  return tags.value.filter((tag) => selected.has(Number(tag.id)));
});

async function toggleTagHidden(tag: LibraryTag) {
  await libConfig.toggleTagHidden(Number(tag.id));
  await emitEvent("library-total-refreshed", { source: "hidden-filters" });
}

async function showAllTags() {
  await libConfig.showAllTags();
  await emitEvent("library-total-refreshed", { source: "hidden-filters" });
}

function createSmartFromTag(tag: LibraryTag) {
  void emitEvent("create-smart-album-from-draft", buildSmartDraftFromTags([tag], false));
}

function createSmartFromSelectedTags() {
  if (!selectedTagObjects.value.length) return;
  void emitEvent(
    "create-smart-album-from-draft",
    buildSmartDraftFromTags(selectedTagObjects.value, Boolean(libConfig.tag.matchAny)),
  );
}

async function saveTagAsCollection(tag: LibraryTag) {
  const fileIds = await getQueryFileIds(makeDefaultQueryParams({ tagId: Number(tag.id) }));
  const created = await saveFilesAsNewCollection(tag.name, fileIds || []);
  if (created) {
    toast.success(t("collection.saved_as_collection_toast", {
      name: created.collection.name,
      count: created.addedCount.toLocaleString(),
    }));
  }
}

async function saveSelectedTagsAsCollection() {
  if (!selectedTagObjects.value.length) return;
  const ids = selectedTagObjects.value.map((tag) => Number(tag.id));
  const matchAny = Boolean(libConfig.tag.matchAny);
  const fileIds = await getQueryFileIds(
    makeDefaultQueryParams({ tagFilter: { ids, any: matchAny } }),
  );
  const baseName = selectedTagObjects.value.map((tag) => tag.name).join(matchAny ? " / " : " + ");
  const created = await saveFilesAsNewCollection(baseName, fileIds || []);
  if (created) {
    toast.success(t("collection.saved_as_collection_toast", {
      name: created.collection.name,
      count: created.addedCount.toLocaleString(),
    }));
  }
}

function tagMenu(tag: LibraryTag): any[] {
  const hidden = isTagHidden(tag.id);
  return [
    {
      label: t("msgbox.new_tag.title"),
      icon: IconTagAdd,
      action: () => clickAddTag(tag.id),
    },
    { label: t("menu.tag.rename"), icon: IconRename, action: () => edit(tag) },
    {
      label: hidden ? t("tag.show") : t("tag.hide"),
      icon: hidden ? IconUnhide : IconHide,
      action: () => { void toggleTagHidden(tag); },
    },
    { label: "-", action: null },
    {
      label: t("collection.create_smart_collection"),
      icon: IconFolderCog,
      action: () => createSmartFromTag(tag),
    },
    {
      label: t("collection.save_as_collection"),
      icon: IconCollection,
      action: () => { void saveTagAsCollection(tag); },
    },
    { label: "-", action: null },
    {
      label: t("tag.delete_tag"),
      icon: IconTrash,
      action: () => prepareDelete(tag),
    },
  ];
}
let dragStart: {
  id: number;
  pointer: number;
  x: number;
  y: number;
  parent: number | null;
} | null = null;
let excludedDropIds = new Set<number>();
let suppressDragClick = false;
function startDrag(event: PointerEvent, tag: LibraryTag) {
  if (libConfig.tag.multi || event.metaKey || event.ctrlKey) return;
  if (
    event.button !== 0 ||
    !event.isPrimary ||
    editor.value ||
    saving.value ||
    (event.target as Element).closest("button, input, [role=button]")
  )
    return;
  stopDrag();
  dragStart = {
    id: tag.id,
    pointer: event.pointerId,
    x: event.clientX,
    y: event.clientY,
    parent: tag.parent_id,
  };
  window.addEventListener("pointermove", dragMove);
  window.addEventListener("pointerup", finishDrag);
  window.addEventListener("pointercancel", stopDrag);
  window.addEventListener("blur", stopDrag);
  window.addEventListener("keydown", cancelDrag);
}
function dragMove(event: PointerEvent) {
  if (!dragStart || event.pointerId !== dragStart.pointer) return;
  if (dragged.value === null) {
    if (
      Math.hypot(event.clientX - dragStart.x, event.clientY - dragStart.y) < 5
    )
      return;
    dragged.value = dragStart.id;
    excludedDropIds = subtreeIds(tags.value, dragStart.id);
    suppressDragClick = true;
    uiStore.pushInputHandler("TagDrag");
  }
  event.preventDefault();
  const element = document.elementFromPoint(event.clientX, event.clientY);
  dropTarget.value = undefined;
  if (!element || !tagList.value?.contains(element)) return;
  if (element.closest("[data-tag-drop-root]")) {
    if (dragStart.parent !== null) dropTarget.value = null;
    return;
  }
  const target = element.closest<HTMLElement>("[data-tag-drop-id]");
  if (!target) return;
  const id = Number(target.dataset.tagDropId);
  if (!excludedDropIds.has(id) && id !== dragStart.parent)
    dropTarget.value = id;
}
function finishDrag(event: PointerEvent) {
  if (!dragStart || event.pointerId !== dragStart.pointer) return;
  if (dragged.value !== null) dragMove(event);
  const id = dragged.value;
  const parent = dropTarget.value;
  stopDrag();
  if (id !== null && parent !== undefined) void move(id, parent);
}
function cancelDrag(event: KeyboardEvent) {
  if (event.key === "Escape") stopDrag();
}
function stopDrag() {
  dragStart = null;
  dragged.value = null;
  dropTarget.value = undefined;
  uiStore.removeInputHandler("TagDrag");
  window.removeEventListener("pointermove", dragMove);
  window.removeEventListener("pointerup", finishDrag);
  window.removeEventListener("pointercancel", stopDrag);
  window.removeEventListener("blur", stopDrag);
  window.removeEventListener("keydown", cancelDrag);
  if (suppressDragClick)
    setTimeout(() => {
      suppressDragClick = false;
    }, 0);
}
async function move(id: number, parent: number | null) {
  if (saving.value) return;
  if (parent !== null && subtreeIds(tags.value, id).has(parent)) {
    toast.error(t("tag.invalid_move"));
    return;
  }
  saving.value = true;
  try {
    await moveTag(id, parent);
    await load();
    reveal(id);
    await notifyChanged();
  } catch {
    toast.error(t("tag.move_failed"));
  } finally {
    saving.value = false;
  }
}
async function prepareDelete(tag: LibraryTag) {
  if (saving.value) return;
  const library = libConfig._libraryId;
  try {
    const info = await getTagSubtreeInfo(tag.id);
    if (!disposed && library === libConfig._libraryId && info.tag_count > 0)
      deleteTarget.value = { ...tag, ...info };
  } catch {
    toast.error(t("tag.load_failed"));
  }
}
async function confirmDelete() {
  if (!deleteTarget.value || saving.value) return;
  saving.value = true;
  try {
    const removedIds = subtreeIds(tags.value, deleteTarget.value.id);
    if (!(await deleteTag(deleteTarget.value.id)))
      throw new Error("delete failed");
    const remainingHidden = (libConfig.hiddenTagIds || []).filter(
      (id: number) => !removedIds.has(Number(id)),
    );
    const hadHiddenRemoved = remainingHidden.length !== (libConfig.hiddenTagIds || []).length;
    deleteTarget.value = null;
    if (hadHiddenRemoved) {
      await libConfig.applyHiddenFilters({ tagIds: remainingHidden });
      await emitEvent("library-total-refreshed", { source: "hidden-filters" });
    }
    await load();
    await notifyChanged();
  } catch {
    toast.error(t("tag.delete_failed"));
  } finally {
    saving.value = false;
  }
}
async function notifyChanged() {
  await emitEvent("tags-changed", { source: tagsChangeSource });
  if (
    config.main.sidebarIndex === SIDEBAR.TAG &&
    libConfig.activePane === "main"
  )
    libConfig.tag.activateTick++;
}
let request = 0;
let disposed = false;
let unlisten: (() => void) | undefined;
async function load() {
  const current = ++request;
  const library = libConfig._libraryId;
  try {
    const [all, counts] = await Promise.all([getAllTags(0), getTagCounts()]);
    if (disposed || current !== request || library !== libConfig._libraryId)
      return;
    if (!all || !counts) throw new Error("load failed");
    tags.value = all.map((tag: LibraryTag) => ({
      ...tag,
      count: Number(counts[String(tag.id)] || 0),
    }));
    libConfig.tag.counts = counts;
    const validIds = new Set(tags.value.map(tag => tag.id));
    if (libConfig.tag.previousId != null && !validIds.has(libConfig.tag.previousId)) libConfig.tag.previousId = null;
    libConfig.tag.selectedIds = (libConfig.tag.selectedIds || []).filter((id: number) => validIds.has(id));
    if (!tags.value.some((tag) => tag.id === libConfig.tag.id))
      libConfig.tag.id = tags.value[0]?.id ?? null;
    failed.value = false;
  } catch {
    if (current === request) failed.value = true;
  } finally {
    if (current === request) loading.value = false;
  }
}
useLibraryChangeRefresh(load);
onMounted(async () => {
  window.addEventListener("keydown", handleFilterEscape, true);
  const stop = await listen<{ source?: string; patchedTagId?: number; library?: string }>(
    "tags-changed",
    async (event) => {
      if (event.payload?.source === tagsChangeSource) return;
      await load();
      if (!disposed && config.main.sidebarIndex === SIDEBAR.TAG
        && !(event.payload?.library === libConfig._libraryId && event.payload?.patchedTagId === Number(libConfig.tag.id)))
        libConfig.tag.activateTick++;
    },
  );
  if (disposed) stop();
  else unlisten = stop;
  await load();
  if (libConfig.tag.id != null) reveal(libConfig.tag.id);
});
onBeforeUnmount(() => {
  window.removeEventListener("keydown", handleFilterEscape, true);
  stopDrag();
  disposed = true;
  request++;
  unlisten?.();
});
watch(
  () => libConfig._libraryId,
  () => {
    editor.value = null;
    deleteTarget.value = null;
    stopDrag();
    void load();
  },
);
watch(
  () => [config.main.sidebarIndex, libConfig.activePane],
  () => {
    if (config.main.sidebarIndex === SIDEBAR.TAG) void load();
  },
);
defineExpose({ clickAddTag });
</script>

<style scoped>
.tag-filter-modes-slot { width: 64px; }
.tag-filter-modes-enter-active,
.tag-filter-modes-leave-active { transition: width 200ms ease, opacity 200ms ease; }
.tag-filter-modes-enter-from,
.tag-filter-modes-leave-to { width: 0; opacity: 0; }
@media (prefers-reduced-motion: reduce) {
  .tag-filter-modes-enter-active,
  .tag-filter-modes-leave-active { transition: none; }
}
</style>
