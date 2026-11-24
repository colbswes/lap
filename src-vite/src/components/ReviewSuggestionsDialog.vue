<template>
  <ModalDialog
    :title="$t('menu.person.review_title', { name: personName })"
    :width="640"
    @cancel="handleClose"
  >
    <div class="flex flex-col gap-3">
      <p class="text-xs text-base-content/60 leading-relaxed">
        {{ $t('menu.person.review_hint') }}
      </p>

      <div class="min-h-48 max-h-[60vh] overflow-y-auto rounded-box border border-base-content/10 bg-base-100/35 p-2">
        <div v-if="isLoading" class="h-48 flex items-center justify-center">
          <span class="loading loading-spinner loading-sm text-primary"></span>
        </div>

        <div
          v-else-if="suggestions.length === 0"
          class="h-48 px-6 flex flex-col items-center justify-center text-center gap-1.5 text-xs text-base-content/50"
        >
          <IconPerson class="w-6 h-6 opacity-35" />
          <span>{{ $t('menu.person.review_none') }}</span>
        </div>

        <div v-else class="grid grid-cols-[repeat(auto-fill,minmax(96px,1fr))] gap-2">
          <div
            v-for="item in suggestions"
            :key="item.face_id"
            class="flex flex-col gap-1 rounded-box p-1 bg-base-200/50 ring-1 ring-base-content/5"
          >
            <div class="aspect-square rounded-box overflow-hidden bg-base-300 flex items-center justify-center">
              <img
                v-if="item.thumbnail"
                :src="'data:image/jpeg;base64,' + item.thumbnail"
                class="w-full h-full object-cover"
                draggable="false"
              />
              <span v-else-if="item.thumbnail === null" class="loading loading-spinner loading-xs text-base-content/30"></span>
              <IconPerson v-else class="w-6 h-6 text-base-content/30" />
            </div>
            <div class="text-[10px] text-base-content/45 truncate px-0.5" :title="item.person_name || ''">
              {{ item.person_id === person.id ? $t('menu.person.review_auto') : (item.person_name || $t('menu.person.unknown_face')) }}
            </div>
            <div class="grid grid-cols-2 gap-1">
              <button
                type="button"
                class="h-6 rounded-box text-[11px] font-medium bg-base-100/60 hover:bg-error/15 hover:text-error transition-colors cursor-pointer disabled:opacity-40"
                :disabled="busy.has(item.face_id)"
                @click="reject(item)"
              >
                {{ $t('menu.person.review_no') }}
              </button>
              <button
                type="button"
                class="h-6 rounded-box text-[11px] font-medium bg-primary/15 text-primary hover:bg-primary/25 transition-colors cursor-pointer disabled:opacity-40"
                :disabled="busy.has(item.face_id)"
                @click="accept(item)"
              >
                {{ $t('menu.person.review_yes') }}
              </button>
            </div>
          </div>
        </div>
      </div>

      <p v-if="errorMessage" class="text-xs text-error">{{ errorMessage }}</p>

      <div class="flex items-center gap-3 pt-1">
        <span class="text-xs text-base-content/45 tabular-nums">
          {{ $t('menu.person.review_progress', { confirmed: confirmedCount, rejected: rejectedCount }) }}
        </span>
        <div class="flex-1"></div>
        <button type="button" class="t-button-default" :disabled="isBulkSaving" @click="handleClose">
          {{ $t('menu.person.review_done') }}
        </button>
        <button
          type="button"
          class="t-button-primary"
          :disabled="suggestions.length === 0 || isBulkSaving || busy.size > 0"
          @click="acceptAll"
        >
          <span v-if="isBulkSaving" class="loading loading-spinner loading-xs mr-1.5"></span>
          {{ $t('menu.person.review_confirm_all', { count: suggestions.length }) }}
        </button>
      </div>
    </div>
  </ModalDialog>
</template>

<script setup lang="ts">
import { ref, computed, reactive, onMounted, onUnmounted } from 'vue';
import { useI18n } from 'vue-i18n';
import { emit as tauriEmit } from '@tauri-apps/api/event';
import { getFaceSuggestions, getFaceThumbnail, assignFaceToPerson, rejectFaceSuggestion } from '@/common/api';
import { PERSONS_CHANGED_EVENT } from '@/common/personContext';
import { useUIStore } from '@/stores/uiStore';
import { IconPerson } from '@/common/icons';
import ModalDialog from '@/components/ModalDialog.vue';

interface Suggestion {
  face_id: number;
  file_id: number;
  person_id: number | null;
  person_name: string | null;
  similarity: number;
  // undefined: not requested, null: loading, '': unavailable
  thumbnail?: string | null;
}

const props = defineProps<{ person: { id: number; name?: string | null } }>();
const emit = defineEmits<{ (e: 'close', payload: { changed: boolean }): void }>();

const { t } = useI18n();
const uiStore = useUIStore();

const PAGE = 48;
const suggestions = ref<Suggestion[]>([]);
const isLoading = ref(true);
const isBulkSaving = ref(false);
const busy = reactive(new Set<number>());
const errorMessage = ref('');
const confirmedCount = ref(0);
const rejectedCount = ref(0);
let mounted = true;

const personName = computed(() => props.person.name?.trim() || t('menu.person.unnamed'));
const changed = computed(() => confirmedCount.value + rejectedCount.value > 0);

async function load() {
  isLoading.value = true;
  errorMessage.value = '';
  try {
    const result: Suggestion[] = (await getFaceSuggestions(props.person.id, PAGE)) || [];
    if (!mounted) return;
    suggestions.value = result.map(s => ({ ...s, thumbnail: null }));
    for (const item of suggestions.value) void loadThumbnail(item);
  } catch (error) {
    errorMessage.value = String(error);
  } finally {
    isLoading.value = false;
  }
}

async function loadThumbnail(item: Suggestion) {
  const data = await getFaceThumbnail(item.face_id);
  const current = suggestions.value.find(s => s.face_id === item.face_id);
  if (current) current.thumbnail = data || '';
}

function drop(item: Suggestion) {
  suggestions.value = suggestions.value.filter(s => s.face_id !== item.face_id);
  // Keep the queue going: fetch the next batch once this one is reviewed.
  if (suggestions.value.length === 0 && !isBulkSaving.value) void load();
}

async function accept(item: Suggestion) {
  busy.add(item.face_id);
  try {
    await assignFaceToPerson(item.face_id, { personId: props.person.id } as any);
    confirmedCount.value++;
    drop(item);
  } catch (error) {
    errorMessage.value = String(error);
  } finally {
    busy.delete(item.face_id);
  }
}

async function reject(item: Suggestion) {
  busy.add(item.face_id);
  try {
    await rejectFaceSuggestion(item.face_id, props.person.id);
    rejectedCount.value++;
    drop(item);
  } catch (error) {
    errorMessage.value = String(error);
  } finally {
    busy.delete(item.face_id);
  }
}

async function acceptAll() {
  isBulkSaving.value = true;
  errorMessage.value = '';
  try {
    for (const item of [...suggestions.value]) {
      await accept(item);
      if (errorMessage.value) break;
    }
  } finally {
    isBulkSaving.value = false;
  }
  if (suggestions.value.length === 0 && !errorMessage.value) void load();
}

function handleClose() {
  if (isBulkSaving.value) return;
  if (changed.value) void tauriEmit(PERSONS_CHANGED_EVENT, { personId: props.person.id });
  emit('close', { changed: changed.value });
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.defaultPrevented || !uiStore.isInputActive('ReviewSuggestionsDialog')) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    handleClose();
  }
}

onMounted(() => {
  uiStore.pushInputHandler('ReviewSuggestionsDialog');
  window.addEventListener('keydown', handleKeyDown);
  void load();
});

onUnmounted(() => {
  mounted = false;
  window.removeEventListener('keydown', handleKeyDown);
  uiStore.removeInputHandler('ReviewSuggestionsDialog');
});
</script>
