<template>
  <ModalDialog
    :title="$t('menu.person.merge_dialog_title')"
    :width="480"
    @cancel="handleCancel"
  >
    <div class="flex flex-col gap-3">
      <p class="text-xs text-base-content/60 leading-relaxed">
        {{ $t('menu.person.merge_dialog_hint') }}
      </p>

      <!-- Visual Source -> Target Merge Preview -->
      <div class="grid grid-cols-[1fr_auto_1fr] items-center gap-2 p-2.5 rounded-box bg-base-300/60 border border-base-content/10">
        <!-- Source -->
        <div class="flex items-center gap-2 min-w-0">
          <div class="w-9 h-9 rounded-full overflow-hidden bg-base-300 ring-1 ring-base-content/10 shrink-0 flex items-center justify-center">
            <img
              v-if="sourcePerson.thumbnail"
              :src="'data:image/jpeg;base64,' + sourcePerson.thumbnail"
              class="w-full h-full object-cover"
            />
            <IconPerson v-else class="w-5 h-5 text-base-content/30" />
          </div>
          <div class="min-w-0 flex-1">
            <div class="text-xs font-medium text-base-content truncate">
              {{ getDisplayName(sourcePerson) }}
            </div>
            <div v-if="sourcePerson.count != null" class="text-[11px] tabular-nums text-base-content/45">
              {{ Number(sourcePerson.count).toLocaleString() }}
            </div>
          </div>
        </div>

        <IconRight class="w-4 h-4 text-base-content/40 shrink-0" />

        <!-- Target -->
        <div class="flex items-center gap-2 min-w-0">
          <div
            :class="[
              'w-9 h-9 rounded-full overflow-hidden shrink-0 flex items-center justify-center',
              selectedTarget
                ? 'bg-base-300 ring-1 ring-primary/50'
                : 'bg-base-200/70 border border-dashed border-base-content/20',
            ]"
          >
            <img
              v-if="selectedTarget?.thumbnail"
              :src="'data:image/jpeg;base64,' + selectedTarget.thumbnail"
              class="w-full h-full object-cover"
            />
            <IconPerson v-else class="w-5 h-5 text-base-content/30" />
          </div>
          <div class="min-w-0 flex-1">
            <div
              :class="[
                'text-xs truncate',
                selectedTarget ? 'font-medium text-base-content' : 'text-base-content/40 italic',
              ]"
            >
              {{ selectedTarget ? getDisplayName(selectedTarget) : $t('menu.person.merge_select_target') }}
            </div>
            <div v-if="selectedTarget?.count != null" class="text-[11px] tabular-nums text-base-content/45">
              {{ Number(selectedTarget.count).toLocaleString() }}
            </div>
          </div>
        </div>
      </div>

      <!-- Search Input -->
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
          :placeholder="$t('menu.person.search')"
          class="w-full min-w-0 bg-transparent border-none focus:ring-0 px-2 text-sm placeholder-base-content/35 focus:outline-none"
          @focus="isSearchFocused = true"
          @blur="isSearchFocused = false"
        />
        <button
          v-if="searchQuery"
          type="button"
          class="mr-1 p-1 rounded-box text-base-content/35 hover:text-base-content/70 cursor-pointer"
          @click="searchQuery = ''"
        >
          <IconClose class="w-4 h-4" />
        </button>
      </div>

      <!-- Candidate People List -->
      <div
        class="min-h-40 max-h-64 overflow-y-auto rounded-box border border-base-content/10 bg-base-100/35 divide-y divide-base-content/5"
        @scroll="handleListScroll"
      >
        <div v-if="isLoading" class="h-40 flex items-center justify-center">
          <span class="loading loading-spinner loading-sm text-primary"></span>
        </div>

        <div
          v-else-if="candidates.length === 0"
          class="h-40 px-6 flex flex-col items-center justify-center text-center gap-1.5 text-xs text-base-content/50"
        >
          <IconPerson class="w-6 h-6 opacity-35" />
          <span>{{ $t('menu.person.merge_no_candidates') }}</span>
        </div>

        <button
          v-else
          v-for="person in candidates"
          :key="person.id"
          type="button"
          :class="[
            'w-full px-3 py-2 flex items-center gap-2.5 text-left transition-colors cursor-pointer',
            selectedTarget?.id === person.id
              ? 'bg-primary/15 text-base-content'
              : 'hover:bg-base-100/60 text-base-content/80',
          ]"
          @click="chooseTarget(person)"
          @dblclick="chooseTargetAndConfirm(person)"
        >
          <div class="w-8 h-8 rounded-full overflow-hidden bg-base-300/80 ring-1 ring-base-content/10 shrink-0 flex items-center justify-center">
            <img
              v-if="person.thumbnail"
              :src="'data:image/jpeg;base64,' + person.thumbnail"
              class="w-full h-full object-cover"
            />
            <IconPerson v-else class="w-5 h-5 text-base-content/30" />
          </div>
          <span class="min-w-0 flex-1 text-sm truncate">
            {{ getDisplayName(person) }}
          </span>
          <span v-if="person.count != null" class="text-xs tabular-nums text-base-content/45 shrink-0">
            {{ Number(person.count).toLocaleString() }}
          </span>
          <IconOk
            v-if="selectedTarget?.id === person.id"
            class="w-4 h-4 text-primary shrink-0"
          />
        </button>

        <div v-if="isLoadingMore" class="py-2 flex justify-center">
          <span class="loading loading-spinner loading-xs text-primary"></span>
        </div>
      </div>

      <!-- Resulting Name -->
      <div class="flex flex-col gap-1">
        <label class="text-xs text-base-content/60" for="merge-person-name">
          {{ $t('menu.person.merge_resulting_name') }}
        </label>
        <input
          id="merge-person-name"
          v-model="resultingName"
          type="text"
          maxlength="255"
          :disabled="!selectedTarget || isMerging"
          :placeholder="$t('menu.person.merge_name_placeholder')"
          class="input input-sm w-full bg-base-100/60 border-base-content/15 focus:border-primary text-sm"
          @input="hasEditedName = true"
          @keydown.enter.prevent="handleConfirm"
        />
      </div>

      <p v-if="errorMessage" class="text-xs text-error">
        {{ errorMessage }}
      </p>

      <!-- Footer -->
      <div class="flex items-center justify-end gap-3 pt-1">
        <button
          type="button"
          class="t-button-default"
          :disabled="isMerging"
          @click="handleCancel"
        >
          {{ $t('msgbox.cancel') }}
        </button>
        <button
          type="button"
          class="t-button-primary"
          :disabled="!selectedTarget || isMerging"
          @click="handleConfirm"
        >
          <span v-if="isMerging" class="loading loading-spinner loading-xs mr-1.5"></span>
          {{ $t('menu.person.merge_confirm') }}
        </button>
      </div>
    </div>
  </ModalDialog>
</template>

<script setup lang="ts">
import { ref, computed, watch, onMounted, onUnmounted, nextTick } from 'vue';
import { useI18n } from 'vue-i18n';
import { config } from '@/common/config';
import { getPersonsPage } from '@/common/api';
import { PERSON_PAGE_SIZE } from '@/common/constants';
import { resolveMergedPersonName } from '@/common/personMerge';
import { useUIStore } from '@/stores/uiStore';
import { IconPerson, IconRight, IconSearch, IconClose, IconOk } from '@/common/icons';
import ModalDialog from '@/components/ModalDialog.vue';

export interface MergePersonSummary {
  id: number;
  name?: string | null;
  count?: number | null;
  thumbnail?: string | null;
}

const props = defineProps<{
  sourcePerson: MergePersonSummary;
  isMerging?: boolean;
  errorMessage?: string;
}>();

const emit = defineEmits<{
  (e: 'cancel'): void;
  (e: 'confirm', payload: { targetPerson: MergePersonSummary; newName: string | null }): void;
}>();

const { t } = useI18n();
const uiStore = useUIStore();

const searchInputRef = ref<HTMLInputElement | null>(null);
const searchQuery = ref('');
const isSearchFocused = ref(false);
const loadedPersons = ref<MergePersonSummary[]>([]);
const isLoading = ref(true);
const isLoadingMore = ref(false);
const hasMore = ref(false);
const offset = ref(0);
const selectedTarget = ref<MergePersonSummary | null>(null);
const resultingName = ref('');
const hasEditedName = ref(false);

let loadRequest = 0;
let searchTimer: ReturnType<typeof setTimeout> | null = null;
let mounted = true;

const candidates = computed(() =>
  loadedPersons.value.filter((person) => person.id !== props.sourcePerson.id),
);

function getDisplayName(person: MergePersonSummary | null | undefined): string {
  return person?.name?.trim() || t('menu.person.unnamed');
}

function chooseTarget(person: MergePersonSummary) {
  selectedTarget.value = person;
  if (!hasEditedName.value) {
    resultingName.value = resolveMergedPersonName(props.sourcePerson.name, person.name);
  }
}

function chooseTargetAndConfirm(person: MergePersonSummary) {
  chooseTarget(person);
  handleConfirm();
}

async function loadCandidates(reset = true) {
  if (!reset && (!hasMore.value || isLoadingMore.value || isLoading.value)) return;
  const requestId = reset ? ++loadRequest : loadRequest;

  if (reset) {
    isLoading.value = true;
    loadedPersons.value = [];
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
    if (page) {
      const incoming: MergePersonSummary[] = Array.isArray(page.persons) ? page.persons : [];
      loadedPersons.value = reset ? incoming : [...loadedPersons.value, ...incoming];
      offset.value += incoming.length;
      hasMore.value = Boolean(page.has_more);
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
  void loadCandidates(false);
}

function handleCancel() {
  if (props.isMerging) return;
  emit('cancel');
}

function handleConfirm() {
  if (!selectedTarget.value || props.isMerging) return;
  const trimmed = resultingName.value.trim();
  emit('confirm', {
    targetPerson: selectedTarget.value,
    newName: trimmed.length > 0 ? trimmed : null,
  });
}

function handleKeyDown(event: KeyboardEvent) {
  if (event.defaultPrevented || !uiStore.isInputActive('MergePersonDialog') || props.isMerging) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    handleCancel();
  }
}

watch(searchQuery, () => {
  if (searchTimer) clearTimeout(searchTimer);
  loadRequest++;
  loadedPersons.value = [];
  offset.value = 0;
  hasMore.value = false;
  isLoading.value = true;
  isLoadingMore.value = false;
  searchTimer = setTimeout(() => {
    searchTimer = null;
    void loadCandidates(true);
  }, 150);
});

onMounted(() => {
  uiStore.pushInputHandler('MergePersonDialog');
  window.addEventListener('keydown', handleKeyDown);
  void loadCandidates(true);
  void nextTick(() => {
    setTimeout(() => searchInputRef.value?.focus(), 50);
  });
});

onUnmounted(() => {
  mounted = false;
  loadRequest++;
  if (searchTimer) clearTimeout(searchTimer);
  window.removeEventListener('keydown', handleKeyDown);
  uiStore.removeInputHandler('MergePersonDialog');
});
</script>
