<template>
  <ModalDialog
    :title="$t('scope.dialog_title')"
    :width="520"
    @cancel="$emit('close')"
  >
    <div class="flex flex-col gap-3">
      <p class="text-xs text-base-content/60 leading-relaxed">
        {{ $t('scope.dialog_hint') }}
      </p>

      <!-- Scope selector tabs -->
      <div class="grid grid-cols-2 gap-1 p-1 rounded-box bg-base-300/70 border border-base-content/5">
        <button
          v-for="tab in editableScopes"
          :key="tab.id"
          type="button"
          :class="[
            'h-8 px-3 rounded-[calc(var(--radius-box)-3px)] text-xs font-medium transition-colors cursor-pointer flex items-center justify-between',
            selectedTab === tab.id
              ? 'bg-base-100 text-primary shadow-xs'
              : 'text-base-content/65 hover:text-base-content hover:bg-base-100/40',
          ]"
          @click="selectedTab = tab.id"
        >
          <span>{{ tab.label }}</span>
          <span class="text-[11px] tabular-nums opacity-65">
            {{ selectedCountFor(tab.id) }}
          </span>
        </button>
      </div>

      <!-- Album / root folder checklist -->
      <div class="min-h-40 max-h-72 overflow-y-auto rounded-box border border-base-content/10 bg-base-100/35 divide-y divide-base-content/5">
        <div v-if="isLoading" class="h-40 flex items-center justify-center">
          <span class="loading loading-spinner loading-sm text-primary"></span>
        </div>

        <div
          v-else-if="albums.length === 0"
          class="h-40 px-6 flex flex-col items-center justify-center text-center gap-2 text-xs text-base-content/55"
        >
          <IconFolders class="w-6 h-6 opacity-40" />
          <span>{{ $t('scope.no_folders_in_library') }}</span>
        </div>

        <label
          v-else
          v-for="album in albums"
          :key="album.id"
          class="px-3 py-2.5 flex items-center gap-3 hover:bg-base-100/50 cursor-pointer transition-colors"
        >
          <input
            type="checkbox"
            class="checkbox checkbox-sm checkbox-primary"
            :checked="isChecked(album.id)"
            @change="toggleAlbum(album.id)"
          />
          <IconFolder class="w-4 h-4 shrink-0 text-base-content/50" />
          <div class="min-w-0 flex-1">
            <div class="text-sm text-base-content truncate">{{ album.name }}</div>
            <div class="text-[11px] text-base-content/45 truncate">{{ album.path }}</div>
          </div>
          <div class="flex items-center gap-1.5 shrink-0">
            <span
              v-if="isInOtherScope(album.id)"
              class="px-1.5 py-0.5 rounded-full text-[10px] bg-base-300/80 text-base-content/55"
            >
              {{ otherScopeLabel }}
            </span>
            <span
              v-if="visibleCounts[album.id] != null"
              class="text-[11px] tabular-nums text-base-content/40"
            >
              {{ Number(visibleCounts[album.id]).toLocaleString() }}
            </span>
          </div>
        </label>
      </div>

      <!-- Footer -->
      <div class="flex items-center justify-between pt-1">
        <span class="text-xs text-base-content/50">
          {{ $t('scope.folders_assigned', selectedCountFor(selectedTab)) }}
        </span>
        <div class="flex items-center gap-2">
          <button
            v-if="libConfig.collectionScope.activeId !== selectedTab"
            type="button"
            class="btn btn-sm btn-ghost text-xs"
            @click="switchToSelectedTab"
          >
            {{ activeTabSwitchLabel }}
          </button>
          <button
            type="button"
            class="btn btn-sm btn-primary text-xs"
            @click="$emit('close')"
          >
            {{ $t('scope.done') }}
          </button>
        </div>
      </div>
    </div>
  </ModalDialog>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { libConfig } from '@/common/config';
import { useUIStore } from '@/stores/uiStore';
import { getAllAlbums, getAlbumVisibleCounts } from '@/common/api';
import {
  SCOPE_EVERYDAY,
  SCOPE_FAMILY,
  isAlbumInScope,
  getScopeById,
  normalizeCollectionScopeState,
} from '@/common/collectionScopes';
import { IconFolder, IconFolders } from '@/common/icons';
import ModalDialog from '@/components/ModalDialog.vue';

const props = defineProps<{
  initialScopeId?: string;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'changed', scopeId?: string): void;
}>();

const { t } = useI18n();
const uiStore = useUIStore();

const isLoading = ref(true);
const albums = ref<any[]>([]);
const visibleCounts = ref<Record<number, number>>({});

const selectedTab = ref<string>(
  props.initialScopeId === SCOPE_FAMILY ? SCOPE_FAMILY : SCOPE_EVERYDAY
);

const editableScopes = computed(() => [
  { id: SCOPE_EVERYDAY, label: t('scope.everyday') },
  { id: SCOPE_FAMILY, label: t('scope.family') },
]);

const normalizedScope = computed(() => normalizeCollectionScopeState(libConfig.collectionScope));

const otherScopeId = computed(() =>
  selectedTab.value === SCOPE_EVERYDAY ? SCOPE_FAMILY : SCOPE_EVERYDAY
);
const otherScopeLabel = computed(() =>
  otherScopeId.value === SCOPE_EVERYDAY ? t('scope.everyday_short') : t('scope.family_short')
);
const activeTabSwitchLabel = computed(() =>
  selectedTab.value === SCOPE_EVERYDAY ? t('scope.everyday') : t('scope.family')
);

function selectedCountFor(scopeId: string): number {
  const validIds = new Set(albums.value.map((a) => Number(a.id)));
  const scope = getScopeById(normalizedScope.value, scopeId);
  if (!scope) return 0;
  if (albums.value.length === 0) return scope.albumIds.length;
  return scope.albumIds.filter((id) => validIds.has(id)).length;
}

function isChecked(albumId: number): boolean {
  return isAlbumInScope(normalizedScope.value, selectedTab.value, albumId);
}

function isInOtherScope(albumId: number): boolean {
  return isAlbumInScope(normalizedScope.value, otherScopeId.value, albumId);
}

async function toggleAlbum(albumId: number) {
  await libConfig.toggleAlbumInScope(selectedTab.value, albumId);
  emit('changed');
}

async function switchToSelectedTab() {
  await libConfig.setActiveScope(selectedTab.value);
  emit('changed', selectedTab.value);
}

onMounted(async () => {
  uiStore.pushInputHandler('CollectionScopeDialog');
  try {
    const [loadedAlbums, counts] = await Promise.all([
      getAllAlbums(false),
      getAlbumVisibleCounts(),
    ]);
    albums.value = Array.isArray(loadedAlbums) ? loadedAlbums : [];
    visibleCounts.value = counts || {};
  } finally {
    isLoading.value = false;
  }
});

onUnmounted(() => {
  uiStore.removeInputHandler('CollectionScopeDialog');
});
</script>
