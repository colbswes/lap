<template>
  <ModalDialog
    :title="$t('apple_photos.dialog_title')"
    :width="560"
    :hideClose="isRunning"
    @cancel="handleClose"
  >
    <div class="flex flex-col gap-3">
      <div class="flex items-start justify-between gap-2">
        <p class="text-xs text-base-content/60 leading-relaxed flex-1">
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

      <div
        v-if="toolStatus && !toolStatus.available"
        class="p-2.5 rounded-box bg-error/10 border border-error/25 text-xs text-error"
      >
        {{ toolStatus.error }}
      </div>

      <!-- Destination folder -->
      <div class="flex flex-col gap-1">
        <label class="text-xs font-medium text-base-content/80">
          {{ $t('apple_photos.destination') }}
        </label>
        <div class="flex items-center gap-2">
          <input
            v-model="destPath"
            type="text"
            :disabled="isRunning"
            class="input input-sm flex-1 bg-base-100/60 border border-base-content/15 text-xs"
            placeholder="~/Pictures/Lap-Everyday"
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

      <!-- Target scope + Versions row -->
      <div class="grid grid-cols-2 gap-3">
        <div class="flex flex-col gap-1">
          <label class="text-xs font-medium text-base-content/80">
            {{ $t('apple_photos.target_scope') }}
          </label>
          <select
            v-model="targetScopeId"
            :disabled="isRunning"
            class="select select-sm bg-base-100/60 border border-base-content/15 text-xs"
          >
            <option :value="SCOPE_EVERYDAY">{{ $t('scope.everyday') }}</option>
            <option :value="SCOPE_FAMILY">{{ $t('scope.family') }}</option>
            <option :value="SCOPE_EVERYTHING">{{ $t('scope.everything') }}</option>
          </select>
        </div>

        <div class="flex flex-col gap-1">
          <label class="text-xs font-medium text-base-content/80">
            {{ $t('apple_photos.versions_label') }}
          </label>
          <select
            v-model="versions"
            :disabled="isRunning"
            class="select select-sm bg-base-100/60 border border-base-content/15 text-xs"
          >
            <option value="edited-prefer">{{ $t('apple_photos.versions_edited_prefer') }}</option>
            <option value="both">{{ $t('apple_photos.versions_both') }}</option>
            <option value="originals">{{ $t('apple_photos.versions_originals') }}</option>
          </select>
        </div>
      </div>

      <!-- Core checkboxes -->
      <div class="grid grid-cols-2 gap-x-3 gap-y-1.5 pt-0.5">
        <label class="flex items-center gap-2 text-xs text-base-content/75 cursor-pointer">
          <input v-model="preserveLivePhotos" type="checkbox" class="checkbox checkbox-xs checkbox-primary" :disabled="isRunning" />
          <span>{{ $t('apple_photos.preserve_live_photos') }}</span>
        </label>
        <label class="flex items-center gap-2 text-xs text-base-content/75 cursor-pointer">
          <input v-model="autoIndexInLap" type="checkbox" class="checkbox checkbox-xs checkbox-primary" :disabled="isRunning" />
          <span>{{ $t('apple_photos.auto_index') }}</span>
        </label>
        <label class="flex items-center gap-2 text-xs text-base-content/75 cursor-pointer">
          <input v-model="downloadMissing" type="checkbox" class="checkbox checkbox-xs checkbox-primary" :disabled="isRunning" />
          <span>{{ $t('apple_photos.download_missing') }}</span>
        </label>
        <label class="flex items-center gap-2 text-xs text-base-content/75 cursor-pointer">
          <input v-model="dryRun" type="checkbox" class="checkbox checkbox-xs checkbox-primary" :disabled="isRunning" />
          <span>{{ $t('apple_photos.dry_run') }}</span>
        </label>
      </div>

      <!-- Optional sample & filter section -->
      <div class="rounded-box border border-base-content/10 bg-base-100/25 overflow-hidden">
        <button
          type="button"
          class="w-full px-3 py-2 flex items-center justify-between text-xs font-medium text-base-content/70 hover:text-base-content cursor-pointer"
          @click="showFilters = !showFilters"
        >
          <span>{{ $t('apple_photos.filters_toggle') }}</span>
          <IconRight
            class="w-4 h-4 transition-transform"
            :class="showFilters ? 'rotate-90' : ''"
          />
        </button>

        <div v-if="showFilters" class="px-3 pb-3 pt-1 grid grid-cols-2 gap-2.5 border-t border-base-content/5">
          <div class="flex flex-col gap-1">
            <label class="text-[11px] text-base-content/60">{{ $t('apple_photos.limit_label') }}</label>
            <input
              v-model="limitInput"
              type="number"
              min="1"
              :disabled="isRunning"
              class="input input-xs bg-base-100/60 border border-base-content/15 text-xs"
              :placeholder="$t('apple_photos.limit_placeholder')"
            />
          </div>
          <div class="flex flex-col gap-1">
            <label class="text-[11px] text-base-content/60">{{ $t('apple_photos.from_date_label') }}</label>
            <input
              v-model="fromDate"
              type="text"
              :disabled="isRunning"
              class="input input-xs bg-base-100/60 border border-base-content/15 text-xs"
              placeholder="YYYY-MM-DD"
            />
          </div>
          <div class="flex flex-col gap-1">
            <label class="text-[11px] text-base-content/60">{{ $t('apple_photos.album_label') }}</label>
            <input
              v-model="albumFilter"
              type="text"
              :disabled="isRunning"
              class="input input-xs bg-base-100/60 border border-base-content/15 text-xs"
              :placeholder="$t('apple_photos.album_placeholder')"
            />
          </div>
          <div class="flex flex-col gap-1">
            <label class="text-[11px] text-base-content/60">{{ $t('apple_photos.library_label') }}</label>
            <input
              v-model="libraryPath"
              type="text"
              :disabled="isRunning"
              class="input input-xs bg-base-100/60 border border-base-content/15 text-xs"
              :placeholder="$t('apple_photos.library_placeholder')"
            />
          </div>
        </div>
      </div>

      <!-- Live progress log -->
      <div
        v-if="isRunning || progressLines.length > 0"
        ref="logContainerRef"
        class="max-h-32 overflow-y-auto rounded-box bg-base-300/80 border border-base-content/10 p-2 font-mono text-[11px] text-base-content/70 space-y-0.5 select-text"
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
        class="p-3 rounded-box border flex flex-col gap-2"
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

        <div class="grid grid-cols-5 gap-1.5 text-center">
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

        <p v-if="exportResult.errorMessage" class="text-[11px] text-error break-words">
          {{ exportResult.errorMessage }}
        </p>
        <p v-if="indexedNotice" class="text-[11px] text-success">
          {{ indexedNotice }}
        </p>
      </div>

      <!-- Error banner -->
      <div v-if="runError" class="p-2.5 rounded-box bg-error/10 border border-error/25 text-xs text-error break-words">
        {{ runError }}
      </div>

      <!-- Footer -->
      <div class="flex items-center justify-end gap-2 pt-1">
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
import {
  SCOPE_EVERYDAY,
  SCOPE_FAMILY,
  SCOPE_EVERYTHING,
  isAlbumInScope,
} from '@/common/collectionScopes';
import { normalizePathForCompare, getFolderName } from '@/common/utils';
import { IconRight } from '@/common/icons';
import ModalDialog from '@/components/ModalDialog.vue';

const emit = defineEmits<{
  (e: 'close'): void;
  (e: 'imported'): void;
}>();

const { t } = useI18n();
const uiStore = useUIStore();

const toolStatus = ref<any>(null);
const destPath = ref('');
const targetScopeId = ref<string>(SCOPE_EVERYDAY);
const versions = ref<'edited-prefer' | 'both' | 'originals'>('edited-prefer');
const preserveLivePhotos = ref(true);
const downloadMissing = ref(false);
const autoIndexInLap = ref(true);
const dryRun = ref(false);
const showFilters = ref(false);
const limitInput = ref('');
const fromDate = ref('');
const albumFilter = ref('');
const libraryPath = ref('');

const isRunning = ref(false);
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

function scopeLabel(scopeId: string): string {
  if (scopeId === SCOPE_EVERYDAY) return t('scope.everyday');
  if (scopeId === SCOPE_FAMILY) return t('scope.family');
  return t('scope.everything');
}

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
    album = await addAlbum(destination, name, 'Exported from Apple Photos');
  }

  if (!album?.id) return;

  if (
    targetScopeId.value !== SCOPE_EVERYTHING &&
    !isAlbumInScope(libConfig.collectionScope, targetScopeId.value, Number(album.id))
  ) {
    await libConfig.toggleAlbumInScope(targetScopeId.value, Number(album.id));
  }

  if (targetScopeId.value !== SCOPE_EVERYTHING) {
    await libConfig.setActiveScope(targetScopeId.value);
  }

  config.leftPanel.show = true;
  await tauriEmit('albums-refreshed', { libraryId: libConfig._libraryId });
  await tauriEmit('library-total-refreshed', { source: 'collection-scope' });
  await indexAlbum(Number(album.id));
  indexedNotice.value = t('apple_photos.indexed_notice', { scope: scopeLabel(targetScopeId.value) });
  emit('imported');
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
