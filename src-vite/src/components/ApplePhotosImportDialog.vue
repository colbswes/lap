<template>
  <ModalDialog
    :title="$t('apple_photos.dialog_title')"
    :width="580"
    :hideClose="isRunning"
    @cancel="handleClose"
  >
    <div class="flex flex-col gap-3 min-w-0">
      <div class="flex items-start justify-between gap-2 min-w-0">
        <p class="text-xs text-base-content/60 leading-relaxed flex-1 min-w-0">
          {{ $t('apple_photos.subtitle') }}
        </p>
        <span
          v-if="toolStatus"
          class="px-2 py-0.5 rounded-full text-[10px] font-medium shrink-0 border"
          :class="toolStatus.available
            ? 'bg-success/10 text-success border-success/25'
            : 'bg-error/10 text-error border-error/25'"
        >
          {{ toolStatus.available
            ? $t('apple_photos.runner_ready', { runner: toolStatus.runnerLabel })
            : $t('apple_photos.runner_missing') }}
        </span>
      </div>

      <!-- Explicit Mode Switch: Import vs Dry Run Preview -->
      <div class="grid grid-cols-2 gap-1 p-1 rounded-box bg-base-300/70 border border-base-content/10">
        <button
          type="button"
          :disabled="isRunning"
          :class="[
            'h-7 px-3 rounded-[calc(var(--radius-box)-3px)] text-xs font-medium transition-colors cursor-pointer',
            !dryRun
              ? 'bg-base-100 text-primary shadow-xs'
              : 'text-base-content/60 hover:text-base-content hover:bg-base-100/40',
          ]"
          @click="dryRun = false"
        >
          {{ $t('apple_photos.mode_import') }}
        </button>
        <button
          type="button"
          :disabled="isRunning"
          :class="[
            'h-7 px-3 rounded-[calc(var(--radius-box)-3px)] text-xs font-medium transition-colors cursor-pointer',
            dryRun
              ? 'bg-base-100 text-primary shadow-xs'
              : 'text-base-content/60 hover:text-base-content hover:bg-base-100/40',
          ]"
          @click="dryRun = true"
        >
          {{ $t('apple_photos.mode_dry_run') }}
        </button>
      </div>

      <p
        v-if="dryRun"
        class="px-2.5 py-1.5 rounded-box bg-info/10 border border-info/25 text-[11px] text-info"
      >
        {{ $t('apple_photos.dry_run_hint') }}
      </p>

      <div
        v-if="toolStatus && !toolStatus.available"
        class="p-2.5 rounded-box bg-error/10 border border-error/25 text-xs text-error"
      >
        {{ toolStatus.error }}
      </div>

      <!-- Destination folder -->
      <div class="flex flex-col gap-1 min-w-0">
        <label class="text-xs font-medium text-base-content/80">
          {{ $t('apple_photos.destination') }}
        </label>
        <div class="flex items-center gap-2 min-w-0">
          <input
            v-model="destPath"
            type="text"
            :disabled="isRunning"
            class="input input-sm flex-1 min-w-0 bg-base-100/60 border border-base-content/15 text-xs"
            placeholder="~/Pictures/Apple Photos Export"
          />
          <button
            type="button"
            class="btn btn-sm btn-ghost border border-base-content/15 text-xs shrink-0"
            :disabled="isRunning"
            @click="chooseDestination"
          >
            {{ $t('apple_photos.browse') }}
          </button>
        </div>
      </div>

      <!-- Versions row -->
      <div class="flex flex-col gap-1 min-w-0">
        <label class="text-xs font-medium text-base-content/80">
          {{ $t('apple_photos.versions_label') }}
        </label>
        <select
          v-model="versions"
          :disabled="isRunning"
          class="select select-sm w-full min-w-0 bg-base-100/60 border border-base-content/15 text-xs"
        >
          <option value="edited-prefer">{{ $t('apple_photos.versions_edited_prefer') }}</option>
          <option value="both">{{ $t('apple_photos.versions_both') }}</option>
          <option value="originals">{{ $t('apple_photos.versions_originals') }}</option>
        </select>
      </div>

      <!-- Core checkboxes -->
      <div class="flex flex-col gap-1.5 pt-0.5 min-w-0">
        <label class="flex items-center gap-2 text-xs text-base-content/80 cursor-pointer min-w-0">
          <input v-model="preserveLivePhotos" type="checkbox" class="checkbox checkbox-xs checkbox-primary shrink-0" :disabled="isRunning" />
          <span class="truncate">{{ $t('apple_photos.preserve_live_photos') }}</span>
        </label>
        <label class="flex items-center gap-2 text-xs text-base-content/80 cursor-pointer min-w-0">
          <input v-model="excludeShared" type="checkbox" class="checkbox checkbox-xs checkbox-primary shrink-0" :disabled="isRunning" />
          <span class="truncate">{{ $t('apple_photos.exclude_shared') }}</span>
        </label>
        <label class="flex items-center gap-2 text-xs text-base-content/80 cursor-pointer min-w-0">
          <input v-model="downloadMissing" type="checkbox" class="checkbox checkbox-xs checkbox-primary shrink-0" :disabled="isRunning" />
          <span class="truncate">{{ $t('apple_photos.download_missing') }}</span>
        </label>
        <label
          v-if="!dryRun"
          class="flex items-center gap-2 text-xs text-base-content/80 cursor-pointer min-w-0"
        >
          <input v-model="autoIndexInLap" type="checkbox" class="checkbox checkbox-xs checkbox-primary shrink-0" :disabled="isRunning" />
          <span class="truncate">{{ $t('apple_photos.auto_index') }}</span>
        </label>
      </div>

      <!-- Optional filter section -->
      <div class="rounded-box border border-base-content/10 bg-base-100/25 overflow-hidden min-w-0">
        <button
          type="button"
          class="w-full px-3 py-2 flex items-center justify-between text-xs font-medium text-base-content/75 hover:text-base-content cursor-pointer"
          @click="showFilters = !showFilters"
        >
          <span>{{ $t('apple_photos.filters_toggle') }}</span>
          <IconRight
            class="w-4 h-4 shrink-0 transition-transform"
            :class="showFilters ? 'rotate-90' : ''"
          />
        </button>

        <div v-if="showFilters" class="px-3 pb-3 pt-2 flex flex-col gap-2.5 border-t border-base-content/5 min-w-0">
          <div class="grid grid-cols-2 gap-2.5 min-w-0">
            <div class="flex flex-col gap-1 min-w-0">
              <label class="text-[11px] text-base-content/60">{{ $t('apple_photos.from_date_label') }}</label>
              <input
                v-model="fromDate"
                type="text"
                :disabled="isRunning"
                class="input input-xs w-full min-w-0 bg-base-100/60 border border-base-content/15 text-xs"
                placeholder="YYYY-MM-DD"
              />
            </div>
            <div class="flex flex-col gap-1 min-w-0">
              <label class="text-[11px] text-base-content/60">{{ $t('apple_photos.album_label') }}</label>
              <input
                v-model="albumFilter"
                type="text"
                :disabled="isRunning"
                class="input input-xs w-full min-w-0 bg-base-100/60 border border-base-content/15 text-xs"
                :placeholder="$t('apple_photos.album_placeholder')"
              />
            </div>
            <div class="flex flex-col gap-1 min-w-0">
              <label class="text-[11px] text-base-content/60">{{ $t('apple_photos.limit_label') }}</label>
              <input
                v-model="limitInput"
                type="number"
                min="1"
                :disabled="isRunning"
                class="input input-xs w-full min-w-0 bg-base-100/60 border border-base-content/15 text-xs"
                :placeholder="$t('apple_photos.limit_placeholder')"
              />
            </div>
            <div class="flex flex-col gap-1 min-w-0">
              <label class="text-[11px] text-base-content/60">{{ $t('apple_photos.library_label') }}</label>
              <input
                v-model="libraryPath"
                type="text"
                :disabled="isRunning"
                class="input input-xs w-full min-w-0 bg-base-100/60 border border-base-content/15 text-xs"
                :placeholder="$t('apple_photos.library_placeholder')"
              />
            </div>
          </div>
          <p v-if="String(limitInput).trim()" class="text-[11px] text-base-content/50 leading-snug">
            {{ $t('apple_photos.limit_hint') }}
          </p>
        </div>
      </div>

      <!-- Live progress log -->
      <div
        v-if="isRunning || progressLines.length > 0"
        ref="logContainerRef"
        class="max-h-32 overflow-y-auto rounded-box bg-base-300/80 border border-base-content/10 p-2 font-mono text-[11px] text-base-content/70 space-y-0.5 select-text min-w-0"
      >
        <div v-if="isRunning && progressLines.length === 0" class="flex items-center gap-2 text-base-content/50">
          <span class="loading loading-spinner loading-xs text-primary"></span>
          <span>{{ $t('apple_photos.running') }}</span>
        </div>
        <div
          v-for="(entry, idx) in progressLines"
          :key="idx"
          :class="entry.stream === 'stderr' ? 'text-warning/90' : ''"
          class="break-all leading-snug"
        >
          {{ entry.line }}
        </div>
      </div>

      <!-- Completion summary -->
      <div
        v-if="exportResult"
        class="p-3 rounded-box border flex flex-col gap-2 min-w-0"
        :class="exportResult.exitCode === 0 && !exportResult.cancelled
          ? 'bg-success/8 border-success/25'
          : (exportResult.cancelled ? 'bg-warning/8 border-warning/25' : 'bg-error/10 border-error/25')"
      >
        <div class="flex items-center justify-between text-xs font-medium">
          <span class="text-base-content">
            {{ exportResult.cancelled
              ? $t('apple_photos.result_cancelled')
              : (exportResult.exitCode === 0
                  ? (exportResult.dryRun ? $t('apple_photos.result_dry_run') : $t('apple_photos.result_complete'))
                  : $t('apple_photos.result_failed')) }}
          </span>
          <span class="text-[11px] tabular-nums text-base-content/55">
            {{ exportResult.elapsedSeconds.toFixed(1) }}s
          </span>
        </div>

        <div
          v-if="!exportResult.cancelled || totalResultCount > 0"
          class="grid grid-cols-5 gap-1.5 text-center"
        >
          <div class="rounded-box bg-base-100/50 py-1 px-1">
            <div class="text-xs font-semibold tabular-nums text-base-content">{{ exportResult.exported }}</div>
            <div class="text-[10px] text-base-content/50">{{ $t('apple_photos.stat_exported') }}</div>
          </div>
          <div class="rounded-box bg-base-100/50 py-1 px-1">
            <div class="text-xs font-semibold tabular-nums text-base-content">{{ exportResult.updated }}</div>
            <div class="text-[10px] text-base-content/50">{{ $t('apple_photos.stat_updated') }}</div>
          </div>
          <div class="rounded-box bg-base-100/50 py-1 px-1">
            <div class="text-xs font-semibold tabular-nums text-base-content">{{ exportResult.skipped }}</div>
            <div class="text-[10px] text-base-content/50">{{ $t('apple_photos.stat_skipped') }}</div>
          </div>
          <div class="rounded-box bg-base-100/50 py-1 px-1">
            <div class="text-xs font-semibold tabular-nums text-base-content">{{ exportResult.missing }}</div>
            <div class="text-[10px] text-base-content/50">{{ $t('apple_photos.stat_missing') }}</div>
          </div>
          <div class="rounded-box bg-base-100/50 py-1 px-1">
            <div class="text-xs font-semibold tabular-nums" :class="exportResult.errors > 0 ? 'text-error' : 'text-base-content'">
              {{ exportResult.errors }}
            </div>
            <div class="text-[10px] text-base-content/50">{{ $t('apple_photos.stat_errors') }}</div>
          </div>
        </div>

        <p v-if="exportResult.cancelled" class="text-[11px] text-warning/90 leading-snug">
          {{ $t('apple_photos.cancelled_hint') }}
        </p>
        <p v-else-if="exportResult.errorMessage" class="text-[11px] text-error break-words">
          {{ exportResult.errorMessage }}
        </p>
        <p v-if="indexedNotice" class="text-[11px] text-success">
          {{ indexedNotice }}
        </p>
      </div>

      <!-- Standalone notice when scanning existing folder -->
      <div
        v-else-if="indexedNotice"
        class="p-2.5 rounded-box bg-success/10 border border-success/25 text-xs text-success"
      >
        {{ indexedNotice }}
      </div>

      <!-- Error banner -->
      <div v-if="runError" class="p-2.5 rounded-box bg-error/10 border border-error/25 text-xs text-error break-words">
        {{ runError }}
      </div>

      <!-- Footer -->
      <div class="flex items-center justify-between gap-2 pt-1">
        <button
          v-if="!isRunning && !dryRun && destPath.trim()"
          type="button"
          class="btn btn-sm btn-ghost border border-base-content/15 text-xs"
          :disabled="isIndexingFolder"
          @click="scanExistingDestination"
        >
          {{ $t('apple_photos.scan_existing') }}
        </button>
        <div v-else></div>

        <div class="flex items-center gap-2">
          <button
            v-if="isRunning"
            type="button"
            class="btn btn-sm btn-warning text-xs"
            @click="handleCancel"
          >
            {{ $t('apple_photos.cancel_export') }}
          </button>
          <template v-else>
            <button
              type="button"
              class="btn btn-sm btn-ghost text-xs"
              @click="handleClose"
            >
              {{ $t('apple_photos.close') }}
            </button>
            <button
              type="button"
              class="btn btn-sm btn-primary text-xs"
              :disabled="!canStart"
              @click="startExport"
            >
              {{ dryRun ? $t('apple_photos.start_dry_run') : $t('apple_photos.start_export') }}
            </button>
          </template>
        </div>
      </div>
    </div>
  </ModalDialog>
</template>

<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted, nextTick } from 'vue';
import { useI18n } from 'vue-i18n';
import { listen, emit as tauriEmit } from '@tauri-apps/api/event';
import { open as openDialog } from '@tauri-apps/plugin-dialog';
import { config, libConfig } from '@/common/config';
import { useUIStore } from '@/stores/uiStore';
import {
  addAlbum,
  cancelApplePhotosExport,
  checkApplePhotosExportReady,
  getAllAlbums,
  indexAlbum,
  runApplePhotosExport,
} from '@/common/api';
import { SIDEBAR } from '@/common/constants';
import { normalizePathForCompare, getFolderName } from '@/common/utils';
import { IconRight } from '@/common/icons';
import ModalDialog from '@/components/ModalDialog.vue';

const props = defineProps<{
  initialPreset?: {
    album?: string;
    dest?: string;
    excludeShared?: boolean;
    downloadMissing?: boolean;
  } | null;
}>();

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'imported'): void;
}>();

const { t } = useI18n();
const uiStore = useUIStore();

const toolStatus = ref<any>(null);
const destPath = ref(props.initialPreset?.dest || '');
const versions = ref<'edited-prefer' | 'both' | 'originals'>('edited-prefer');
const preserveLivePhotos = ref(true);
const excludeShared = ref(props.initialPreset?.excludeShared ?? true);
const downloadMissing = ref(props.initialPreset?.downloadMissing ?? false);
const autoIndexInLap = ref(true);
const dryRun = ref(false);
const showFilters = ref(Boolean(props.initialPreset?.album));
const limitInput = ref('');
const fromDate = ref('');
const albumFilter = ref(props.initialPreset?.album || '');
const libraryPath = ref('');

const isRunning = ref(false);
const isIndexingFolder = ref(false);
const runError = ref('');
const indexedNotice = ref('');
const exportResult = ref<any>(null);
const progressLines = ref<Array<{ line: string; stream: string }>>([]);
const logContainerRef = ref<HTMLElement | null>(null);
let unlistenProgress: (() => void) | null = null;

const canStart = computed(() =>
  !isRunning.value &&
  Boolean(toolStatus.value?.available) &&
  destPath.value.trim().length > 0
);

const totalResultCount = computed(() => {
  if (!exportResult.value) return 0;
  return (
    Number(exportResult.value.exported || 0) +
    Number(exportResult.value.updated || 0) +
    Number(exportResult.value.skipped || 0) +
    Number(exportResult.value.missing || 0) +
    Number(exportResult.value.errors || 0)
  );
});

async function chooseDestination() {
  const picked = await openDialog({
    directory: true,
    multiple: false,
    defaultPath: destPath.value.trim() || undefined,
  });
  if (typeof picked === 'string' && picked.trim()) {
    destPath.value = picked;
  }
}

async function addAndIndexExportedFolder(destination: string) {
  const existingAlbums = (await getAllAlbums(false)) || [];
  const normalizedTarget = normalizePathForCompare(destination);
  let album = existingAlbums.find(
    (item: any) => normalizePathForCompare(item.path) === normalizedTarget
  );

  if (!album) {
    const name = getFolderName(destination) || 'Apple Photos';
    album = await addAlbum(destination, name, 'Exported from Apple Photos', {
      fileTypes: 7,
      smallImageFilter: 0,
      excludedFolders: [],
    });
  }

  if (!album?.id) {
    runError.value = t('apple_photos.index_failed');
    return;
  }

  libConfig.activePane = 'main';
  libConfig.album.id = Number(album.id);
  libConfig.album.folderId = null;
  libConfig.album.folderPath = '';
  libConfig.album.selected = true;
  config.main.sidebarIndex = SIDEBAR.LIBRARY;
  config.leftPanel.show = true;

  await tauriEmit('albums-refreshed', { libraryId: libConfig._libraryId });
  await tauriEmit('library-total-refreshed', { source: 'album-visibility' });
  indexedNotice.value = t('apple_photos.indexed_notice');
  emit('imported');
  void indexAlbum(Number(album.id));
}

async function scanExistingDestination() {
  const target = destPath.value.trim();
  if (!target || isRunning.value || isIndexingFolder.value) return;
  runError.value = '';
  indexedNotice.value = '';
  isIndexingFolder.value = true;
  try {
    await addAndIndexExportedFolder(target);
  } catch (error: any) {
    runError.value = typeof error === 'string' ? error : (error?.message || String(error));
  } finally {
    isIndexingFolder.value = false;
  }
}

async function startExport() {
  if (!canStart.value) return;
  runError.value = '';
  indexedNotice.value = '';
  exportResult.value = null;
  progressLines.value = [];
  isRunning.value = true;

  const parsedLimit = limitInput.value.toString().trim()
    ? Number.parseInt(limitInput.value.toString().trim(), 10)
    : null;

  try {
    const result = await runApplePhotosExport({
      dest: destPath.value.trim(),
      versions: versions.value,
      library: libraryPath.value.trim() || null,
      album: albumFilter.value.trim() || null,
      fromDate: fromDate.value.trim() || null,
      limit: parsedLimit && parsedLimit > 0 ? parsedLimit : null,
      skipLive: !preserveLivePhotos.value,
      excludeShared: excludeShared.value,
      downloadMissing: downloadMissing.value,
      xmpSidecar: false,
      dryRun: dryRun.value,
    });
    exportResult.value = result;

    if (
      result &&
      !result.dryRun &&
      !result.cancelled &&
      result.exitCode === 0 &&
      autoIndexInLap.value
    ) {
      await addAndIndexExportedFolder(result.destination);
    }
  } catch (error: any) {
    runError.value = typeof error === 'string' ? error : (error?.message || String(error));
  } finally {
    isRunning.value = false;
  }
}

async function handleCancel() {
  await cancelApplePhotosExport();
}

function handleClose() {
  if (isRunning.value) return;
  emit('close');
}

onMounted(async () => {
  uiStore.pushInputHandler('ApplePhotosImportDialog');
  unlistenProgress = await listen('apple-photos-export-progress', async (event: any) => {
    const payload = event?.payload;
    if (!payload?.line) return;
    progressLines.value.push({ line: payload.line, stream: payload.stream || 'stdout' });
    if (progressLines.value.length > 200) {
      progressLines.value.splice(0, progressLines.value.length - 200);
    }
    await nextTick();
    if (logContainerRef.value) {
      logContainerRef.value.scrollTop = logContainerRef.value.scrollHeight;
    }
  });
  toolStatus.value = await checkApplePhotosExportReady();
  if (toolStatus.value?.defaultDest && !destPath.value) {
    destPath.value = toolStatus.value.defaultDest;
  }
});

onUnmounted(() => {
  uiStore.removeInputHandler('ApplePhotosImportDialog');
  unlistenProgress?.();
  unlistenProgress = null;
});
</script>
