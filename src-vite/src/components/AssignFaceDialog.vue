<template>
  <ModalDialog
    :title="$t('menu.person.assign_dialog_title')"
    :width="440"
    @cancel="handleCancel"
  >
    <div class="flex flex-col gap-3">
      <div class="flex items-center gap-3">
        <div class="w-14 h-14 rounded-box overflow-hidden bg-base-300 ring-1 ring-base-content/10 shrink-0 flex items-center justify-center">
          <img
            v-if="face.thumbnail"
            :src="'data:image/jpeg;base64,' + face.thumbnail"
            class="w-full h-full object-cover"
            draggable="false"
          />
          <IconPerson v-else class="w-6 h-6 text-base-content/30" />
        </div>
        <p class="text-xs text-base-content/60 leading-relaxed">
          {{ $t('menu.person.assign_dialog_hint') }}
        </p>
      </div>

      <!-- Search / new name -->
      <div
        :class="[
          'h-8 flex items-center rounded-box transition-colors bg-base-100/50',
          isSearchFocused ? 'border-2 border-primary' : 'border border-base-content/15 hover:border-base-content/30',
        ]"
      >
        <IconSearch class="ml-2 w-4 h-4 shrink-0" :class="isSearchFocused ? 'text-primary/70' : 'text-base-content/35'" />
        <input
          ref="searchInputRef"
          v-model="searchQuery"
          type="text"
          maxlength="255"
          :placeholder="$t('menu.person.search')"
          class="w-full min-w-0 bg-transparent border-none focus:ring-0 px-2 text-sm placeholder-base-content/35 focus:outline-none"
          @focus="isSearchFocused = true"
          @blur="isSearchFocused = false"
          @keydown.enter.prevent="handleConfirm"
        />
      </div>

      <!-- Candidates -->
      <div
        class="min-h-40 max-h-64 overflow-y-auto rounded-box border border-base-content/10 bg-base-100/35 divide-y divide-base-content/5"
        @scroll="handleListScroll"
      >
        <button
          v-if="createName"
          type="button"
          :class="[
            'w-full px-3 py-2 flex items-center gap-2.5 text-left transition-colors cursor-pointer',
            selected === null ? 'bg-primary/15 text-base-content' : 'hover:bg-base-100/60 text-base-content/80',
          ]"
          @click="selected = null"
          @dblclick="selected = null; handleConfirm()"
        >
          <div class="w-8 h-8 rounded-full bg-base-200/70 border border-dashed border-base-content/20 shrink-0 flex items-center justify-center">
            <IconAdd class="w-4 h-4 text-base-content/40" />
          </div>
          <span class="min-w-0 flex-1 text-sm truncate">{{ $t('menu.person.assign_create', { name: createName }) }}</span>
          <IconOk v-if="selected === null" class="w-4 h-4 text-primary shrink-0" />
        </button>

        <div v-if="isLoading" class="h-40 flex items-center justify-center">
          <span class="loading loading-spinner loading-sm text-primary"></span>
        </div>

        <template v-else>
          <button
            v-for="person in persons"
            :key="person.id"
            type="button"
            :class="[
              'w-full px-3 py-2 flex items-center gap-2.5 text-left transition-colors cursor-pointer',
              selected?.id === person.id ? 'bg-primary/15 text-base-content' : 'hover:bg-base-100/60 text-base-content/80',
            ]"
            @click="selected = person"
            @dblclick="selected = person; handleConfirm()"
          >
            <div class="w-8 h-8 rounded-full overflow-hidden bg-base-300/80 ring-1 ring-base-content/10 shrink-0 flex items-center justify-center">
              <img
                v-if="person.thumbnail"
                :src="'data:image/jpeg;base64,' + person.thumbnail"
                class="w-full h-full object-cover"
              />
              <IconPerson v-else class="w-5 h-5 text-base-content/30" />
            </div>
            <span class="min-w-0 flex-1 text-sm truncate">{{ displayName(person) }}</span>
            <span v-if="person.count != null" class="text-xs tabular-nums text-base-content/45 shrink-0">
              {{ Number(person.count).toLocaleString() }}
            </span>
            <IconOk v-if="selected?.id === person.id" class="w-4 h-4 text-primary shrink-0" />
          </button>
        </template>

        <div v-if="isLoadingMore" class="py-2 flex justify-center">
          <span class="loading loading-spinner loading-xs text-primary"></span>
        </div>
      </div>

      <p v-if="errorMessage" class="text-xs text-error">{{ errorMessage }}</p>

      <!-- Footer -->
      <div class="flex items-center gap-3 pt-1">
        <button
          v-if="face.person_id"
          type="button"
          class="t-button-default"
          :disabled="isSaving"
          @click="handleReject"
        >
          {{ $t('menu.person.assign_not_person', { name: currentName }) }}
        </button>
        <div class="flex-1"></div>
        <button type="button" class="t-button-default" :disabled="isSaving" @click="handleCancel">
          {{ $t('msgbox.cancel') }}
        </button>
        <button
          type="button"
          class="t-button-primary"
          :disabled="!canConfirm || isSaving"
          @click="handleConfirm"
        >
          <span v-if="isSaving" class="loading loading-spinner loading-xs mr-1.5"></span>
          {{ $t('menu.person.assign_confirm') }}
        </button>
      </div>
    </div>
  </ModalDialog>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue';
import { useI18n } from 'vue-i18n';
import { emit as tauriEmit } from '@tauri-apps/api/event';
import { config } from '@/common/config';
import { getPersonsPage, assignFaceToPerson, removeFaceFromPerson } from '@/common/api';
import { PERSON_PAGE_SIZE } from '@/common/constants';
import { PERSONS_CHANGED_EVENT } from '@/common/personContext';
import { useUIStore } from '@/stores/uiStore';
import { IconPerson, IconSearch, IconOk, IconAdd } from '@/common/icons';
import ModalDialog from '@/components/ModalDialog.vue';

export interface AssignFaceTarget {
  id: number;
  person_id?: number | null;
  person_name?: string | null;
  thumbnail?: string | null;
}

interface PersonSummary {
  id: number;
  name?: string | null;
  count?: number | null;
  thumbnail?: string | null;
}

const props = defineProps<{ face: AssignFaceTarget }>();

const emit = defineEmits<{
  (e: 'cancel'): void;
  (e: 'changed', payload: { personId: number | null; message: string }): void;
}>();

const { t } = useI18n();
const uiStore = useUIStore();

const searchInputRef = ref<HTMLInputElement | null>(null);
const searchQuery = ref('');
const isSearchFocused = ref(false);
const persons = ref<PersonSummary[]>([]);
const isLoading = ref(true);
const isLoadingMore = ref(false);
const hasMore = ref(false);
const offset = ref(0);
// A person from the list, or null for "create the typed name".
const selected = ref<PersonSummary | null>(null);
const isSaving = ref(false);
const errorMessage = ref('');

let loadRequest = 0;
let searchTimer: ReturnType<typeof setTimeout> | null = null;
let mounted = true;

const currentName = computed(() => props.face.person_name?.trim() || t('menu.person.unnamed'));

function displayName(person: PersonSummary) {
  return person.name?.trim() || t('menu.person.unnamed');
}

// Offer to create the typed name unless a listed person already has it.
const createName = computed(() => {
  const name = searchQuery.value.trim();
  if (!name) return '';
  const lower = name.toLowerCase();
  return persons.value.some(p => (p.name || '').trim().toLowerCase() === lower) ? '' : name;
});

const canConfirm = computed(() => Boolean(selected.value) || Boolean(createName.value));

async function loadPersons(reset = true) {
  if (!reset && (!hasMore.value || isLoadingMore.value || isLoading.value)) return;
  const requestId = reset ? ++loadRequest : loadRequest;
  if (reset) {
    isLoading.value = true;
    offset.value = 0;
    hasMore.value = false;
  } else {
    isLoadingMore.value = true;
  }
  try {
    const page = await getPersonsPage({
      sort: config.settings.personSort,
      offset: offset.value,
      limit: PERSON_PAGE_SIZE,
      search: searchQuery.value.trim(),
      refreshSummary: null,
    });
    if (!mounted || requestId !== loadRequest) return;
    const incoming: PersonSummary[] = Array.isArray(page?.persons) ? page.persons : [];
    persons.value = reset ? incoming : [...persons.value, ...incoming];
    offset.value += incoming.length;
    hasMore.value = Boolean(page?.has_more);
    if (reset) {
      // Preselect the current person (so Enter confirms it), else an exact name match.
      const query = searchQuery.value.trim().toLowerCase();
      selected.value = (query
        ? incoming.find(p => (p.name || '').trim().toLowerCase() === query)
        : incoming.find(p => p.id === props.face.person_id)) || (query ? null : selected.value);
    }
  } finally {
    if (requestId === loadRequest) {
      isLoading.value = false;
      isLoadingMore.value = false;
    }
  }
}

function handleListScroll(event: Event) {
  const el = event.currentTarget as HTMLElement;
  if (el.scrollTop + el.clientHeight < el.scrollHeight - 24) return;
  void loadPersons(false);
}

function handleCancel() {
  if (!isSaving.value) emit('cancel');
}

async function handleConfirm() {
  if (!canConfirm.value || isSaving.value) return;
  isSaving.value = true;
  errorMessage.value = '';
  try {
    const target = selected.value;
    const name = target ? displayName(target) : createName.value;
    const personId = await assignFaceToPerson(props.face.id, (target ? { personId: target.id } : { name }) as any);
    void tauriEmit(PERSONS_CHANGED_EVENT, { personId });
    emit('changed', { personId, message: t('menu.person.assign_success', { name }) });
  } catch (error) {
    errorMessage.value = String(error);
  } finally {
    isSaving.value = false;
  }
}

async function handleReject() {
  if (isSaving.value) return;
  isSaving.value = true;
  errorMessage.value = '';
  try {
    const personId = await removeFaceFromPerson(props.face.id);
    void tauriEmit(PERSONS_CHANGED_EVENT, { personId });
    emit('changed', { personId: null, message: t('menu.person.removed_from_person', { count: 1, name: currentName.value }) });
  } catch (error) {
    errorMessage.value = String(error);
  } finally {
    isSaving.value = false;
  }
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.defaultPrevented || !uiStore.isInputActive('AssignFaceDialog') || isSaving.value) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    handleCancel();
  }
}

watch(searchQuery, () => {
  if (searchTimer) clearTimeout(searchTimer);
  loadRequest++;
  isLoading.value = true;
  searchTimer = setTimeout(() => {
    searchTimer = null;
    void loadPersons(true);
  }, 150);
});

onMounted(() => {
  uiStore.pushInputHandler('AssignFaceDialog');
  window.addEventListener('keydown', handleKeyDown);
  void loadPersons(true);
  void nextTick(() => {
    setTimeout(() => searchInputRef.value?.focus(), 50);
  });
});

onUnmounted(() => {
  mounted = false;
  loadRequest++;
  if (searchTimer) clearTimeout(searchTimer);
  window.removeEventListener('keydown', handleKeyDown);
  uiStore.removeInputHandler('AssignFaceDialog');
});
</script>
