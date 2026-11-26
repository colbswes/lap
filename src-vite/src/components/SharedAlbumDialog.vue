<template>
  <ModalDialog
    :title="album.title"
    :width="840"
    :height="620"
    @cancel="handleClose"
  >
    <template #title-actions>
      <div class="flex items-center gap-1.5">
        <button
          type="button"
          class="btn btn-xs btn-ghost border border-base-content/15 text-[11px]"
          @click="handleOpenInPhotos"
        >
          <IconExternal class="w-3.5 h-3.5" />
          {{ $t('apple_photos.shared_album_open_photos') }}
        </button>
      </div>
    </template>

    <div class="flex-1 min-h-0 flex flex-col gap-2">
      <!-- Subtitle bar -->
      <div class="flex items-center justify-between gap-2 text-[11px] text-base-content/55 shrink-0">
        <span class="truncate">
          <template v-if="album.owner">{{ $t('apple_photos.shared_album_by', { owner: album.owner }) }} · </template>
          {{ album.count.toLocaleString() }} items
        </span>
        <span class="truncate text-base-content/45">
          {{ $t('apple_photos.shared_album_preview_hint') }}
        </span>
      </div>

      <!-- Loading -->
      <div v-if="isLoading" class="flex-1 flex items-center justify-center">
        <span class="loading loading-spinner loading-md text-primary"></span>
      </div>

      <!-- Error -->
      <div v-else-if="loadError" class="flex-1 flex items-center justify-center p-6 text-center">
        <p class="text-xs text-error max-w-md break-words">{{ loadError }}</p>
      </div>

      <!-- Empty -->
      <div v-else-if="photos.length === 0" class="flex-1 flex items-center justify-center text-xs text-base-content/50">
        {{ $t('apple_photos.shared_album_empty') }}
      </div>

      <!-- Lightbox preview -->
      <div v-else-if="selectedPhoto" class="flex-1 min-h-0 flex flex-col gap-2">
        <div class="flex items-center justify-between gap-2 shrink-0">
          <button
            type="button"
            class="btn btn-xs btn-ghost text-xs"
            @click="selectedIndex = -1"
          >
            <IconLeft class="w-3.5 h-3.5" />
            Grid ({{ (selectedIndex + 1).toLocaleString() }} / {{ photos.length.toLocaleString() }})
          </button>
          <div class="flex items-center gap-2 min-w-0 text-xs text-base-content/70">
            <span class="truncate font-medium">{{ selectedPhoto.filename }}</span>
            <span v-if="formattedDate(selectedPhoto.date)" class="text-base-content/45 shrink-0">
              {{ formattedDate(selectedPhoto.date) }}
            </span>
            <span
              class="px-1.5 py-0.5 rounded-full text-[10px] shrink-0"
              :class="selectedPhoto.isLocalOriginal ? 'bg-success/15 text-success' : 'bg-base-300 text-base-content/55'"
            >
              {{ selectedPhoto.isLocalOriginal ? $t('apple_photos.shared_album_local_badge') : $t('apple_photos.shared_album_cloud_badge') }}
            </span>
          </div>
          <div class="flex items-center gap-1 shrink-0">
            <button
              type="button"
              class="btn btn-xs btn-ghost"
              :disabled="selectedIndex <= 0"
              @click="selectedIndex = Math.max(0, selectedIndex - 1)"
            >
              <IconLeft class="w-4 h-4" />
            </button>
            <button
              type="button"
              class="btn btn-xs btn-ghost"
              :disabled="selectedIndex >= photos.length - 1"
              @click="selectedIndex = Math.min(photos.length - 1, selectedIndex + 1)"
            >
              <IconRight class="w-4 h-4" />
            </button>
          </div>
        </div>

        <div class="flex-1 min-h-0 rounded-box bg-black/50 flex items-center justify-center overflow-hidden p-2">
          <img
            v-if="selectedPhotoUrl"
            :src="selectedPhotoUrl"
            :alt="selectedPhoto.filename"
            class="max-w-full max-h-full object-contain rounded"
          />
          <div v-else class="text-xs text-base-content/50">
            {{ selectedPhoto.filename }}
          </div>
        </div>
      </div>

      <!-- Thumbnail grid -->
      <div
        v-else
        class="flex-1 min-h-0 overflow-y-auto rounded-box bg-base-300/40 border border-base-content/10 p-2"
      >
        <div class="grid grid-cols-5 gap-2">
          <button
            v-for="(photo, index) in photos"
            :key="photo.uuid"
            type="button"
            class="group relative aspect-square rounded-box overflow-hidden bg-base-200 border border-base-content/5 hover:border-primary/60 transition-colors cursor-pointer"
            @click="selectedIndex = index"
          >
            <img
              v-if="photo.previewPath"
              :src="photo.previewPath"
              :alt="photo.filename"
              loading="lazy"
              decoding="async"
              class="w-full h-full object-cover group-hover:scale-[1.02] transition-transform duration-150"
            />
            <div v-else class="w-full h-full flex items-center justify-center text-[10px] text-base-content/40 p-2 text-center break-all">
              {{ photo.filename }}
            </div>

            <span
              v-if="photo.isMovie"
              class="absolute bottom-1 left-1 px-1.5 py-0.5 rounded bg-black/65 text-[9px] font-medium text-white uppercase"
            >
              Video
            </span>
            <span
              v-if="!photo.isLocalOriginal"
              class="absolute top-1 right-1 p-0.5 rounded-full bg-black/55 text-white/85"
              :title="$t('apple_photos.shared_album_cloud_badge')"
            >
              <IconCloud class="w-3 h-3" />
            </span>
          </button>
        </div>
      </div>
    </div>
  </ModalDialog>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { useUIStore } from '@/stores/uiStore';
import { getAppleSharedAlbumPhotos, openApplePhotosApp } from '@/common/api';
import { IconCloud, IconExternal, IconLeft, IconRight } from '@/common/icons';
import ModalDialog from '@/components/ModalDialog.vue';

export interface SharedAlbumSummary {
  uuid: string;
  title: string;
  owner: string;
  count: number;
  coverPath?: string | null;
}

const props = defineProps<{
  album: SharedAlbumSummary;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
}>();

const uiStore = useUIStore();
const isLoading = ref(true);
const loadError = ref('');
const photos = ref<any[]>([]);
const selectedIndex = ref(-1);

const selectedPhoto = computed(() =>
  selectedIndex.value >= 0 && selectedIndex.value < photos.value.length
    ? photos.value[selectedIndex.value]
    : null
);

// Previews arrive as data URLs from Photos; the large one is fetched (from iCloud if needed) on selection.
const largePreviews = ref<Record<string, string>>({});
watch(selectedPhoto, async item => {
  if (!item || item.isMovie || largePreviews.value[item.uuid]) return;
  try {
    const url = await invoke<string | null>('get_apple_shared_photo_preview', { id: item.uuid });
    if (url) largePreviews.value = { ...largePreviews.value, [item.uuid]: url };
  } catch (error) {
    console.error('Failed to load the Shared Album preview:', error);
  }
});
const selectedPhotoUrl = computed(() => {
  const item = selectedPhoto.value;
  if (!item) return '';
  return largePreviews.value[item.uuid] || item.previewPath || '';
});

function formattedDate(iso: string | null | undefined): string {
  if (!iso) return '';
  try {
    return new Date(iso).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  } catch {
    return '';
  }
}

function handleKeyDown(event: KeyboardEvent) {
  if (selectedIndex.value < 0) return;
  if (event.key === 'ArrowLeft') {
    event.preventDefault();
    selectedIndex.value = Math.max(0, selectedIndex.value - 1);
  } else if (event.key === 'ArrowRight') {
    event.preventDefault();
    selectedIndex.value = Math.min(photos.value.length - 1, selectedIndex.value + 1);
  } else if (event.key === 'Escape') {
    event.preventDefault();
    event.stopPropagation();
    selectedIndex.value = -1;
  }
}

async function handleOpenInPhotos() {
  await openApplePhotosApp();
}

function handleClose() {
  emit('close');
}

onMounted(async () => {
  uiStore.pushInputHandler('SharedAlbumDialog');
  window.addEventListener('keydown', handleKeyDown, true);
  try {
    const items = await getAppleSharedAlbumPhotos(props.album.uuid);
    photos.value = Array.isArray(items) ? items : [];
  } catch (error: any) {
    loadError.value = typeof error === 'string' ? error : (error?.message || String(error));
  } finally {
    isLoading.value = false;
  }
});

onUnmounted(() => {
  window.removeEventListener('keydown', handleKeyDown, true);
  uiStore.removeInputHandler('SharedAlbumDialog');
});
</script>
