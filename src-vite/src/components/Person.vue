<template>
  <div class="sidebar-panel relative overflow-hidden">
    <!-- Face Indexing / Apple Pet Import Progress Overlay -->
    <div v-if="isIndexing || isImportingApplePets" 
      class="absolute inset-0 z-50 bg-base-200/80 backdrop-blur-md"
    >
      <div class="mt-8 px-2 flex flex-col items-center text-base-content/50">
        <IconUpdate class="w-8 h-8 mb-2 animate-spin" />
        <template v-if="isImportingApplePets && !isIndexing">
          <span class="text-sm text-center">
            {{ appleImportProgress.total > 0
              ? $t('menu.person.apple_pets_importing_progress', { current: appleImportProgress.current.toLocaleString(), total: appleImportProgress.total.toLocaleString() })
              : $t('menu.person.apple_pets_importing')
            }}
          </span>
        </template>
        <template v-else>
          <span class="text-sm text-center">
            {{ indexProgress.phase === 'clustering' 
              ? $t('face_index.clustering') 
              : $t('face_index.indexing', { current: indexProgress.current.toLocaleString(), total: indexProgress.total.toLocaleString() }) 
            }}
          </span>
          <span v-if="indexProgress.phase === 'clustering' && clusterProgressText" class="text-xs text-center mt-1">
            {{ clusterProgressText }}
          </span>
          <span v-else-if="indexProgress.faces_found > 0" class="text-xs text-center mt-1">
            {{ $t('face_index.faces_found', { count: indexProgress.faces_found.toLocaleString() }) }}
          </span>
          <button class="btn btn-primary btn-sm mt-4" @click="clickCancelIndex">
            <IconClose class="w-4 h-4" />
            {{ $t('face_index.cancel') }}
          </button>
        </template>
      </div>
    </div>

    <!-- Incomplete Indexing Warning Banner -->
    <div v-if="allPersons.length > 0 && incompleteCount > 0 && !isIndexing" class="flex-none px-2 py-2">
        <div class="p-3 rounded-box flex flex-row items-center gap-2">
          <IconUpdate class="w-5 h-5 shrink-0" />
          <span class="text-xs flex-1">
            {{ $t('face_index.incomplete', { count: incompleteCount.toLocaleString() }) }}
          </span>
          <button class="btn btn-xs btn-primary" @click="clickIndexFaces()">
            {{ $t('face_index.resume') }}
          </button>
        </div>
    </div>

    <div class="sidebar-panel-header">
      <span class="sidebar-panel-header-title flex-1 min-w-0 overflow-hidden text-ellipsis whitespace-nowrap">
        {{ titlebar }}<template v-if="allPersonCount > 0"> ({{ allPersonCount.toLocaleString() }})</template>
      </span>
      <span class="px-1.5 h-5 inline-flex items-center rounded-box text-[10px] font-semibold tracking-[0.08em] text-warning border border-warning/30 bg-warning/10 cursor-default">
        BETA
      </span>

      <SortMenuButton v-model="config.settings.personSort" kind="category" />
      <ContextMenu :menuItems="personPanelMenuItems" :iconMenu="IconMore" :smallIcon="true" />
    </div>

    <!-- People / Pets -->
    <div class="mx-1 mb-2 px-1 shrink-0">
      <div class="grid grid-cols-2 p-0.5 rounded-box bg-base-100/40 border border-base-content/10 text-xs">
        <button
          v-for="option in PERSON_KINDS"
          :key="option.kind"
          type="button"
          :class="[
            'h-6 rounded-box transition-colors cursor-pointer',
            personKind === option.kind ? 'bg-base-300 text-base-content font-medium' : 'text-base-content/55 hover:text-base-content',
          ]"
          @click="personKind = option.kind"
        >
          {{ $t(option.label) }}
        </button>
      </div>
    </div>

    <div class="mx-1 mb-2 px-1 shrink-0">
      <div
        :class="[
          'h-8 flex items-center rounded-box transition-colors bg-base-100/40',
          isPersonSearchFocused ? 'border-2 border-primary' : 'border border-base-content/10 hover:border-base-content/30',
          personSearchDisabled ? 'opacity-50' : '',
        ]"
      >
        <IconSearch class="ml-2 w-4 h-4 shrink-0" :class="isPersonSearchFocused ? 'text-primary/70' : 'text-base-content/30'" />
        <input
          v-model="personSearch"
          type="text"
          :disabled="personSearchDisabled"
          :placeholder="$t('menu.person.search')"
          class="w-full min-w-0 bg-transparent border-none focus:ring-0 px-2 text-sm placeholder-base-content/30 focus:outline-none disabled:opacity-50"
          @focus="isPersonSearchFocused = true"
          @blur="isPersonSearchFocused = false"
        />
        <button
          v-if="personSearch"
          type="button"
          :disabled="personSearchDisabled"
          class="mr-1 p-1 rounded-box text-base-content/30 hover:text-base-content/70 disabled:opacity-30"
          @click="personSearch = ''"
        >
          <IconClose class="w-4 h-4" />
        </button>
      </div>
    </div>

    <!-- Person List -->
    <div
      v-if="allPersons.length > 0"
      ref="personListRef"
      class="grow overflow-x-hidden overflow-y-auto"
      @scroll="handlePersonListScroll"
    >
      <ul>
        <li v-for="person in sortedPersons" :key="person.id" :id="'person-' + person.id">
          <div
            :data-person-drop-id="person.id"
            :class="[
              'sidebar-item gap-2 group select-none',
              selectedPerson && selectedPerson.id === person.id && !isRenamingPerson ? 'sidebar-item-selected' : 'sidebar-item-hover',
              {
                'opacity-50': draggedPersonId === person.id,
                'bg-primary/10 ring-1 ring-inset ring-primary/50': dropTargetPersonId === person.id,
              },
            ]"
            @pointerdown="startPersonDrag($event, person)"
            @dragstart.prevent
            @click="handlePersonClick(person)"
            @contextmenu.prevent.stop="(e: MouseEvent) => handlePersonContextMenu(person, e)"
          >
            <!-- Face thumbnail -->
            <div class="w-8 h-8 rounded-full overflow-hidden bg-base-300/70 ring-1 ring-base-content/5 shrink-0 flex items-center justify-center">
              <img 
                v-if="person.thumbnail" 
                :src="'data:image/jpeg;base64,' + person.thumbnail" 
                class="w-full h-full object-cover pointer-events-none"
              />
              <IconPerson v-else class="w-5 h-5 text-base-content/30" />
            </div>
            
            <!-- Name input or display -->
            <input v-if="selectedPerson && selectedPerson.id === person.id && isRenamingPerson"
              ref="personInputRef"
              type="text"
              maxlength="255"
              class="input px-1 flex-1 focus:border text-base"
              v-model="person.name"
              @keydown.enter.prevent="handleRenamePerson"
              @keydown.esc.prevent="cancelRenamePerson"
              @blur="handleRenamePerson"
            />
            <template v-else>
              <span class="sidebar-item-label">
                {{ getPersonDisplayName(person) }}
              </span>
              <span
                v-if="dropTargetPersonId === person.id"
                class="ml-auto px-1.5 py-0.5 rounded-box text-[10px] font-medium bg-primary/15 text-primary border border-primary/30 shrink-0"
              >
                {{ $t('menu.person.drop_to_merge') }}
              </span>
              <div v-else class="ml-auto flex flex-row items-center text-base-content/30">
                <span v-if="person.count" class="sidebar-item-count shrink-0">
                  {{ person.count.toLocaleString() }}
                </span>
                <div :class="[
                    selectedPerson?.id === person.id ? '' : 'hidden group-hover:flex'
                  ]"
                >
                  <ContextMenu
                    :ref="(el: any) => { if (el) personContextMenus[person.id] = el }"
                    :iconMenu="IconMore"
                    :menuItems="getMoreMenuItems(person)"
                    :smallIcon="true"
                  />
                </div>
              </div>
            </template>
          </div>
        </li>
      </ul>
    </div>

    <div v-else-if="isLoadingPersons" class="mt-2 px-2 flex flex-col items-center justify-center text-base-content/30">
      <span class="text-sm text-center">{{ $t('tooltip.loading') }}</span>
    </div>

    <div v-else-if="personSearch" class="sidebar-empty text-sm">
      <span class="text-center">{{ $t('tooltip.not_found.person') }}</span>
    </div>

    <!-- No Persons Found Message -->
    <div v-else-if="!isIndexing && incompleteCount > 0" class="mt-2 px-2 flex flex-col items-center justify-center text-base-content/30">
      <span class="text-sm text-center">{{ $t('face_index.incomplete', { count: incompleteCount.toLocaleString() }) }}</span>
      <button class="btn btn-primary btn-sm mt-4 rounded-box" @click="clickIndexFaces()">
        <IconUpdate class="w-4 h-4" />
        {{ $t('face_index.resume') }}
      </button>
    </div>

    <div v-else-if="!isIndexing" class="mt-2 px-2 flex flex-col items-center justify-center text-base-content/30">
      <span class="text-sm text-center">{{ $t('tooltip.not_found.person') }}</span>
    </div>

  </div>

  <!-- Delete person confirmation -->
  <MessageBox
    v-if="showDeletePersonMsgbox"
    :title="$t('msgbox.delete_person.title')"
    :message="`${$t('msgbox.delete_person.content', { person: getPersonDisplayName(deleteTargetPerson || selectedPerson) })}`"
    :OkText="$t('msgbox.delete_person.ok')"
    :cancelText="$t('msgbox.cancel')"
    :warningOk="true"
    @ok="clickDeletePerson"
    @cancel="cancelDeletePerson"
  />

  <!-- Merge person picker dialog -->
  <MergePersonDialog
    v-if="mergeDialogSource"
    :source-person="mergeDialogSource"
    :kind="personKind"
    :is-merging="isMergingPersons"
    :error-message="mergeDialogError"
    @cancel="closeMergeDialog"
    @confirm="onMergeDialogConfirm"
  />

  <!-- Drag-to-merge confirmation -->
  <MessageBox
    v-if="pendingDragMerge"
    :title="$t('msgbox.merge_person.title')"
    :message="$t('msgbox.merge_person.content', {
      source: getPersonDisplayName(pendingDragMerge.source),
      target: getPersonDisplayName(pendingDragMerge.target),
      resultName: pendingDragMerge.resultName || getPersonDisplayName(pendingDragMerge.target),
    })"
    :OkText="$t('msgbox.merge_person.ok')"
    :cancelText="$t('msgbox.cancel')"
    :is-loading="isMergingPersons"
    @ok="confirmDragMerge"
    @cancel="cancelDragMerge"
  />

  <!-- Rename-to-existing-name merge confirmation -->
  <MessageBox
    v-if="pendingRenameMerge"
    :title="$t('msgbox.merge_person.title')"
    :message="$t('msgbox.merge_person.rename_duplicate', {
      source: pendingRenameMerge.originalName || getPersonDisplayName(pendingRenameMerge.source),
      target: getPersonDisplayName(pendingRenameMerge.target),
      count: Number(pendingRenameMerge.target.count || 0).toLocaleString(),
    })"
    :OkText="$t('msgbox.merge_person.ok')"
    :thirdText="$t('msgbox.merge_person.keep_separate')"
    :cancelText="$t('msgbox.cancel')"
    :is-loading="isMergingPersons"
    @ok="confirmRenameMerge"
    @third="confirmRenameKeepSeparate"
    @cancel="cancelRenameMerge"
  />

  <!-- Reset faces confirmation -->
  <MessageBox
    v-if="showResetFacesMsgbox"
    :title="$t('msgbox.reset_faces.title')"
    :message="$t('msgbox.reset_faces.content')"
    :OkText="$t('msgbox.reset_faces.ok')"
    :cancelText="$t('msgbox.cancel')"
    :warningOk="true"
    @ok="onResetFacesConfirm"
    @cancel="showResetFacesMsgbox = false"
  />

  <teleport to="body">
    <transition name="fade">
      <div
        v-if="isBetaTooltipVisible && config.settings.showToolTip"
        ref="betaTooltipRef"
        class="fixed z-1000 px-2 py-1 text-xs whitespace-nowrap rounded-box bg-neutral text-neutral-content shadow-lg pointer-events-none"
        :style="betaTooltipStyle"
      >
        {{ $t('tooltip.beta.person') }}
      </div>
    </transition>
  </teleport>
</template>

<script setup lang="ts">
import { ref, onMounted, onUnmounted, computed, nextTick, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { config, libConfig } from '@/common/config';
import { getPersonsPage, renamePerson, deletePerson, mergePersons, findPersonByName, indexFaces, cancelFaceIndex, isFaceIndexing, listenFaceIndexProgress, listenFaceIndexFinished, listenClusterProgress, resetFaces, getFaceStats, importApplePets } from '@/common/api';
import { resolveMergedPersonName } from '@/common/personMerge';
import { PERSONS_CHANGED_EVENT } from '@/common/personContext';
import { listen } from '@tauri-apps/api/event';
import { useToast } from '@/common/toast';
import { useUIStore } from '@/stores/uiStore';
import { useLibraryChangeRefresh } from '@/composables/useLibraryChangeRefresh';
import { SIDEBAR, PERSON_PAGE_SIZE } from '@/common/constants';
import { 
  IconPerson, 
  IconMore, 
  IconRename, 
  IconGroup,
  IconTrash,
  IconUpdate,
  IconClose,
  IconSearch,
} from '@/common/icons';

import ContextMenu from '@/components/ContextMenu.vue';
import MessageBox from '@/components/MessageBox.vue';
import MergePersonDialog from '@/components/MergePersonDialog.vue';
import SortMenuButton from '@/components/SortMenuButton.vue';

const props = defineProps({
  titlebar: {
    type: String,
    required: true
  }
});

const emit = defineEmits(['editDataChanged']);

/// i18n
const { locale, messages } = useI18n();
const localeMsg = computed(() => messages.value[locale.value] as any);
const toast = useToast();
const uiStore = useUIStore();

// persons
const allPersons = ref<any[]>([]);
const selectedPerson = ref<any>(null);
const personListRef = ref<HTMLElement | null>(null);
const isRenamingPerson = ref(false);
const originalPersonName = ref('');
const personInputRef = ref<HTMLInputElement[]>([]);
const isIndexing = ref(false);
const indexProgress = ref({
  current: 0,
  total: 0,
  faces_found: 0,
  phase: 'indexing'
});
const clusterProgress = ref({
  phase: '',
  current: 0,
  total: 0
});
const incompleteCount = ref(0);
const personContextMenus = ref<Record<number, any>>({});
const isLoadingPersons = ref(true);
const isLoadingMorePersons = ref(false);
const hasMorePersons = ref(false);
const allPersonCount = ref(0);
const personSearch = ref('');
const isPersonSearchFocused = ref(false);
// Disable search only for a truly empty library; never lock it on a zero-match
// search (allPersonCount now reflects the query), so the user can always clear it.
const personSearchDisabled = computed(
  () => !isLoadingPersons.value && allPersonCount.value === 0 && !personSearch.value,
);

let personLoadRequest = 0;
let personSearchTimer: ReturnType<typeof setTimeout> | null = null;
let isPersonMounted = true;

function handlePersonContextMenu(person: any, event: MouseEvent) {
  selectPerson(person);
  personContextMenus.value[person.id]?.open?.(event.clientX, event.clientY);
}
const betaBadgeRef = ref<HTMLElement | null>(null);
const betaTooltipRef = ref<HTMLElement | null>(null);
const isBetaTooltipVisible = ref(false);
const betaTooltipStyle = ref<Record<string, string>>({});

// Event listener unsubscribe functions
let unlistenProgress: (() => void) | null = null;
let unlistenFinished: (() => void) | null = null;
let unlistenCluster: (() => void) | null = null;

const sortedPersons = computed(() => allPersons.value);

// Computed property to format cluster progress text using i18n
const { t } = useI18n();
const getPersonDisplayName = (person: any) => person?.name || t('menu.person.unnamed');
const clusterProgressText = computed(() => {
  const { phase, current, total } = clusterProgress.value;
  switch (phase) {
    case 'graph':
      return t('face_index.cluster_graph', { percent: current });
    case 'iterate':
      return t('face_index.cluster_iterate', { current, total });
    case 'converged':
      return t('face_index.cluster_converged', { current });
    case 'assign':
      return t('face_index.cluster_assign', { current, total });
    case 'thumbnail':
      return t('face_index.cluster_thumbnail');
    default:
      return '';
  }
});

const personPanelMenuItems = computed(() => [
  {
    label: localeMsg.value.menu.person.index_faces,
    icon: IconUpdate,
    action: () => clickIndexFaces(),
    disabled: isIndexing.value,
  },
  {
    label: localeMsg.value.menu.person.rescan_faces,
    icon: IconUpdate,
    action: () => clickIndexFaces(true),
    disabled: isIndexing.value,
  },
  {
    label: localeMsg.value.menu.person.import_apple_pets,
    icon: IconUpdate,
    action: () => clickImportApplePets(),
    disabled: isIndexing.value || isImportingApplePets.value,
  },
  { label: "-", action: null },
  {
    label: localeMsg.value.menu.person.reset_index,
    icon: IconTrash,
    action: () => clickResetFaces(),
    disabled: isIndexing.value,
  },
]);

// message boxes & merge dialogs
const showDeletePersonMsgbox = ref(false);
const deleteTargetPerson = ref<any>(null);
const showResetFacesMsgbox = ref(false);
const mergeDialogSource = ref<any>(null);
const mergeDialogError = ref('');
const isMergingPersons = ref(false);
const pendingDragMerge = ref<{ source: any; target: any; resultName: string } | null>(null);
const pendingRenameMerge = ref<{ source: any; target: any; newName: string; originalName: string } | null>(null);

// pointer-based drag-to-merge state (mirrors Tag.vue so it works cleanly in Tauri WKWebView)
const draggedPersonId = ref<number | null>(null);
const dropTargetPersonId = ref<number | null>(null);
let dragStartState: { id: number; pointer: number; x: number; y: number } | null = null;
let suppressDragClick = false;

// more menuitems
const getMoreMenuItems = (person: any) => [
  {
    label: localeMsg.value.menu?.person?.rename || 'Rename',
    icon: IconRename,
    disabled: isIndexing.value || isMergingPersons.value,
    action: () => {
      selectPerson(person);
      isRenamingPerson.value = true;
      originalPersonName.value = person?.name || '';
      nextTick(() => {
        if (personInputRef.value && personInputRef.value[0]) {
          personInputRef.value[0].focus();
          personInputRef.value[0].select();
        }
      });
    }
  },
  {
    label: localeMsg.value.menu?.person?.merge || 'Merge into…',
    icon: IconGroup,
    disabled: isIndexing.value || isMergingPersons.value || (allPersonCount.value <= 1 && !personSearch.value.trim()),
    action: () => {
      mergeDialogError.value = '';
      mergeDialogSource.value = person;
    }
  },
  { label: "-", action: null },
  {
    label: localeMsg.value.menu?.person?.delete || 'Delete',
    icon: IconTrash,
    disabled: isIndexing.value || isMergingPersons.value,
    action: () => {
      deleteTargetPerson.value = person;
      showDeletePersonMsgbox.value = true;
    },
  },
];

onMounted(async () => {
  loadPersons();
  checkFaceStats();
  
  // Check if indexing is already running and restore progress
  const [isRunning, progress] = await isFaceIndexing();
  
  if (isRunning) {
    isIndexing.value = true;
    if (progress) {
      indexProgress.value = progress;
    }
  }
  
  // Set up event listeners for face indexing progress
  unlistenProgress = await listenFaceIndexProgress((event: any) => {
    isIndexing.value = true; // Show overlay when receiving progress events
    indexProgress.value = event.payload;
  });
  
  unlistenFinished = await listenFaceIndexFinished((event: any) => {
    isIndexing.value = false;
    indexProgress.value = { current: 0, total: 0, faces_found: 0, phase: 'indexing' };
    clusterProgress.value = { phase: '', current: 0, total: 0 };
    loadPersons(); // Reload persons after indexing completes
    checkFaceStats();
  });
  
  // Listen for detailed clustering progress
  unlistenCluster = await listenClusterProgress((event: any) => {
    clusterProgress.value = event.payload;
  });
});

watch(() => config.settings.personSort, () => {
  loadPersons();
});

// People and pets are separate lists; switching clears the selection.
const PERSON_KINDS = [
  { kind: 'person', label: 'menu.person.kind_people' },
  { kind: 'pet', label: 'menu.person.kind_pets' },
] as const;
const personKind = computed({
  get: () => (config.settings.personKind === 'pet' ? 'pet' : 'person'),
  set: (value: 'person' | 'pet') => { config.settings.personKind = value; },
});
watch(personKind, () => {
  libConfig.person.id = null;
  libConfig.person.name = null;
  void loadPersons(true);
});

const isImportingApplePets = ref(false);
const appleImportProgress = ref({ current: 0, total: 0 });

// Confirm pets named in Apple Photos, then re-cluster so the names spread.
async function clickImportApplePets() {
  if (isIndexing.value || isImportingApplePets.value) return;
  isImportingApplePets.value = true;
  appleImportProgress.value = { current: 0, total: 0 };
  personKind.value = 'pet';
  let unlistenImportProgress: (() => void) | null = null;
  try {
    unlistenImportProgress = await listen('apple_pet_import_progress', (event: any) => {
      appleImportProgress.value = {
        current: Number(event?.payload?.current ?? 0),
        total: Number(event?.payload?.total ?? 0),
      };
    });
    const result: any = await importApplePets();
    toast.success(t('menu.person.apple_pets_imported', { count: result?.confirmed ?? 0, names: result?.names ?? 0 }));
    if ((result?.missing_detection ?? 0) > 0) {
      toast.info(t('menu.person.apple_pets_missing', { count: result.missing_detection }));
    }
    await loadPersons(true);
    await clickIndexFaces();
  } catch (error) {
    toast.error(String(error));
  } finally {
    if (unlistenImportProgress) unlistenImportProgress();
    isImportingApplePets.value = false;
    appleImportProgress.value = { current: 0, total: 0 };
  }
}

watch(personSearch, () => {
  if (personSearchTimer) clearTimeout(personSearchTimer);
  personLoadRequest++;
  allPersons.value = [];
  hasMorePersons.value = false;
  isLoadingPersons.value = true;
  isLoadingMorePersons.value = false;
  personSearchTimer = setTimeout(() => {
    personSearchTimer = null;
    void loadPersons();
  }, 200);
});

onUnmounted(() => {
  isPersonMounted = false;
  stopPersonDrag();
  personLoadRequest++;
  if (personSearchTimer) clearTimeout(personSearchTimer);
  if (unlistenProgress) unlistenProgress();
  if (unlistenFinished) unlistenFinished();
  if (unlistenCluster) unlistenCluster();
});

async function loadPersons(reset = true, validateSelectedPerson = false) {
  if (!reset && (!hasMorePersons.value || isLoadingMorePersons.value || isLoadingPersons.value)) return;

  const requestId = reset ? ++personLoadRequest : personLoadRequest;
  const libraryId = libConfig._libraryId;
  const search = personSearch.value.trim();
  if (reset) {
    isLoadingPersons.value = true;
    allPersons.value = [];
    hasMorePersons.value = false;
  } else {
    isLoadingMorePersons.value = true;
  }

  try {
    const page = await getPersonsPage({
      kind: personKind.value,
      sort: config.settings.personSort,
      offset: reset ? 0 : allPersons.value.length,
      limit: PERSON_PAGE_SIZE,
      search,
      refreshSummary: validateSelectedPerson
        ? { selectedPersonId: libConfig.person?.id ?? null }
        : null,
    });
    if (!isPersonMounted || requestId !== personLoadRequest || libraryId !== libConfig._libraryId) return;

    if (page) {
      const selectedPersonWasFiltered = validateSelectedPerson && page.selected_person_visible === false;
      if (selectedPersonWasFiltered) {
        selectedPerson.value = null;
        if (libConfig.person) {
          libConfig.person.id = null;
          libConfig.person.name = null;
        }
      }
      allPersons.value = reset
        ? page.persons
        : [...allPersons.value, ...page.persons];
      hasMorePersons.value = page.has_more;
      // `total` is the search-filtered visible count, so the header reflects the query.
      allPersonCount.value = page.total;
      if (allPersons.value.length > 0 && !selectedPersonWasFiltered) {
        const activeId = selectedPerson.value?.id ?? libConfig.person?.id;
        const index = allPersons.value.findIndex(p => p.id === activeId);
        if (index >= 0) {
          selectedPerson.value = allPersons.value[index];
          if (libConfig.person) {
            libConfig.person.id = allPersons.value[index].id;
            libConfig.person.name = allPersons.value[index].name;
          }
        } else if (!selectedPerson.value) {
          selectPerson(allPersons.value[0]);
        }
      }
    } else if (libConfig.person) {
      libConfig.person.id = null;
    }
  } finally {
    if (requestId === personLoadRequest) {
      isLoadingPersons.value = false;
      isLoadingMorePersons.value = false;
    }
  }
}

function handlePersonListScroll(event: Event) {
  const target = event.currentTarget as HTMLElement;
  if (target.scrollTop + target.clientHeight < target.scrollHeight - 24) return;
  void loadPersons(false);
}

function handlePersonClick(person: any) {
  if (suppressDragClick) return;
  selectPerson(person);
}

function selectPerson(person: any) {
  if (isRenamingPerson.value) return;
  selectedPerson.value = person;
  if (!libConfig.person) {
    libConfig.person = { id: null, name: null, activateTick: 0 };
  }
  libConfig.person.id = person.id;
  libConfig.person.name = person.name;
}

function bumpPersonContentRefresh() {
  if (!libConfig.person) {
    libConfig.person = { id: null, name: null, activateTick: 0 };
  }
  libConfig.person.activateTick = Number(libConfig.person.activateTick || 0) + 1;
}

async function applyRenamePerson(person: any, newName: string) {
  const result = await renamePerson(person.id, newName);
  if (result) {
    person.name = newName;
    if (libConfig.person?.id === person.id) {
      libConfig.person.name = newName;
      bumpPersonContentRefresh();
    }
    await loadPersons();
  }
}

async function handleRenamePerson() {
  if (!isRenamingPerson.value) return;

  const person = selectedPerson.value;
  const previousName = originalPersonName.value;
  const newName = person?.name?.trim() || '';
  isRenamingPerson.value = false;

  if (!person || newName.length === 0 || newName === previousName) {
    if (person) {
      person.name = previousName;
    }
    return;
  }

  // If another person already has this exact name, offer to merge instead of creating duplicates.
  const existing = await findPersonByName(newName, person.id, personKind.value);
  if (existing && existing.id !== person.id) {
    person.name = previousName;
    pendingRenameMerge.value = {
      source: person,
      target: existing,
      newName,
      originalName: previousName,
    };
    return;
  }

  await applyRenamePerson(person, newName);
}

function cancelRenamePerson() {
  if (selectedPerson.value) {
    selectedPerson.value.name = originalPersonName.value;
  }
  isRenamingPerson.value = false;
}

async function executeMergePersons(
  sourcePerson: any,
  targetPerson: any,
  newName: string | null = null,
): Promise<boolean> {
  if (!sourcePerson || !targetPerson || sourcePerson.id === targetPerson.id || isMergingPersons.value) {
    return false;
  }

  isMergingPersons.value = true;
  mergeDialogError.value = '';
  const sourceDisplayName = getPersonDisplayName(sourcePerson);

  try {
    const result = await mergePersons([sourcePerson.id], targetPerson.id, newName);
    if (!result?.target) {
      throw new Error(t('menu.person.merge_failed'));
    }

    await libConfig.remapMergedPersonIds([sourcePerson.id], result.target.id);

    selectedPerson.value = result.target;
    if (!libConfig.person) {
      libConfig.person = { id: null, name: null, activateTick: 0 };
    }
    libConfig.person.id = result.target.id;
    libConfig.person.name = result.target.name;
    bumpPersonContentRefresh();

    await loadPersons(true);

    const targetDisplayName = getPersonDisplayName(result.target);
    toast.success(
      t('menu.person.merge_success', {
        source: sourceDisplayName,
        target: targetDisplayName,
      }),
    );
    return true;
  } catch (error: any) {
    const msg = typeof error === 'string' ? error : (error?.message || t('menu.person.merge_failed'));
    mergeDialogError.value = msg;
    toast.error(t('menu.person.merge_failed'));
    return false;
  } finally {
    isMergingPersons.value = false;
  }
}

function closeMergeDialog() {
  if (isMergingPersons.value) return;
  mergeDialogSource.value = null;
  mergeDialogError.value = '';
}

async function onMergeDialogConfirm(payload: { targetPerson: any; newName: string | null }) {
  const source = mergeDialogSource.value;
  if (!source) return;
  const ok = await executeMergePersons(source, payload.targetPerson, payload.newName);
  if (ok) {
    mergeDialogSource.value = null;
    mergeDialogError.value = '';
  }
}

async function confirmDragMerge() {
  const pending = pendingDragMerge.value;
  if (!pending) return;
  const ok = await executeMergePersons(
    pending.source,
    pending.target,
    pending.resultName || null,
  );
  if (ok) {
    pendingDragMerge.value = null;
  }
}

function cancelDragMerge() {
  if (isMergingPersons.value) return;
  pendingDragMerge.value = null;
}

async function confirmRenameMerge() {
  const pending = pendingRenameMerge.value;
  if (!pending) return;
  const ok = await executeMergePersons(pending.source, pending.target, pending.newName);
  if (ok) {
    pendingRenameMerge.value = null;
  }
}

async function confirmRenameKeepSeparate() {
  const pending = pendingRenameMerge.value;
  if (!pending || isMergingPersons.value) return;
  pendingRenameMerge.value = null;
  await applyRenamePerson(pending.source, pending.newName);
}

function cancelRenameMerge() {
  if (isMergingPersons.value) return;
  pendingRenameMerge.value = null;
}

function startPersonDrag(event: PointerEvent, person: any) {
  if (
    event.button !== 0 ||
    !event.isPrimary ||
    isRenamingPerson.value ||
    isIndexing.value ||
    isMergingPersons.value ||
    (event.target as Element).closest('button, input, [role=button]')
  ) {
    return;
  }
  stopPersonDrag();
  dragStartState = {
    id: person.id,
    pointer: event.pointerId,
    x: event.clientX,
    y: event.clientY,
  };
  window.addEventListener('pointermove', onPersonDragMove);
  window.addEventListener('pointerup', onPersonDragEnd);
  window.addEventListener('pointercancel', stopPersonDrag);
  window.addEventListener('blur', stopPersonDrag);
  window.addEventListener('keydown', onPersonDragKeyDown);
}

function onPersonDragMove(event: PointerEvent) {
  if (!dragStartState || event.pointerId !== dragStartState.pointer) return;
  if (draggedPersonId.value === null) {
    if (Math.hypot(event.clientX - dragStartState.x, event.clientY - dragStartState.y) < 6) {
      return;
    }
    draggedPersonId.value = dragStartState.id;
    suppressDragClick = true;
    uiStore.pushInputHandler('PersonDrag');
  }
  event.preventDefault();
  const element = document.elementFromPoint(event.clientX, event.clientY);
  dropTargetPersonId.value = null;
  if (!element || !personListRef.value?.contains(element)) return;
  const targetEl = element.closest<HTMLElement>('[data-person-drop-id]');
  if (!targetEl) return;
  const targetId = Number(targetEl.dataset.personDropId);
  if (Number.isFinite(targetId) && targetId !== draggedPersonId.value) {
    dropTargetPersonId.value = targetId;
  }
}

function onPersonDragEnd(event: PointerEvent) {
  if (!dragStartState || event.pointerId !== dragStartState.pointer) return;
  if (draggedPersonId.value !== null) onPersonDragMove(event);
  const sourceId = draggedPersonId.value;
  const targetId = dropTargetPersonId.value;
  stopPersonDrag();
  if (sourceId !== null && targetId !== null && sourceId !== targetId) {
    const source = allPersons.value.find(p => p.id === sourceId);
    const target = allPersons.value.find(p => p.id === targetId);
    if (source && target) {
      pendingDragMerge.value = {
        source,
        target,
        resultName: resolveMergedPersonName(source.name, target.name),
      };
    }
  }
}

function onPersonDragKeyDown(event: KeyboardEvent) {
  if (event.key === 'Escape') stopPersonDrag();
}

function stopPersonDrag() {
  dragStartState = null;
  draggedPersonId.value = null;
  dropTargetPersonId.value = null;
  uiStore.removeInputHandler('PersonDrag');
  window.removeEventListener('pointermove', onPersonDragMove);
  window.removeEventListener('pointerup', onPersonDragEnd);
  window.removeEventListener('pointercancel', stopPersonDrag);
  window.removeEventListener('blur', stopPersonDrag);
  window.removeEventListener('keydown', onPersonDragKeyDown);
  if (suppressDragClick) {
    setTimeout(() => {
      suppressDragClick = false;
    }, 0);
  }
}

function cancelDeletePerson() {
  showDeletePersonMsgbox.value = false;
  deleteTargetPerson.value = null;
}

async function clickDeletePerson() {
  const target = deleteTargetPerson.value || selectedPerson.value;
  showDeletePersonMsgbox.value = false;
  deleteTargetPerson.value = null;
  if (!target) return;

  const result = await deletePerson(target.id);
  if (result) {
    const index = allPersons.value.findIndex(p => p.id === target.id);
    const wasSelected = selectedPerson.value?.id === target.id;
    allPersons.value = allPersons.value.filter(p => p.id !== target.id);
    allPersonCount.value = Math.max(0, allPersonCount.value - 1);
    if (wasSelected) {
      if (index > 0) {
        selectPerson(allPersons.value[index - 1]);
      } else if (index === 0) {
        if (allPersons.value.length > 0) {
          selectPerson(allPersons.value[0]);
        } else {
          selectedPerson.value = null;
          if (libConfig.person) {
            libConfig.person.id = null;
          }
        }
      } else {
        selectedPerson.value = null;
        if (libConfig.person) {
          libConfig.person.id = null;
        }
      }
    }
  }
}

// Called from title bar context menu
async function clickIndexFaces(rescan = false) {
  if (isIndexing.value) {
    return;
  }
  
  isIndexing.value = true;
  try {
    // Get cluster threshold from array using index
    const face = config.settings.face;
    const thresholdIndex = face?.clusterThresholdIndex ?? 2; // Default: Medium (index 2)
    // Use getter for thresholds to ensure we get the latest values, even if state is old
    const thresholds = config.faceClusterThresholds ?? [0.35, 0.45, 0.55, 0.65];
    const clusterEpsilon = thresholds[thresholdIndex] ?? 0.55;
    console.log('clusterEpsilon', clusterEpsilon);
    await indexFaces(clusterEpsilon, rescan);
    await loadPersons();
    await checkFaceStats();
  } catch (e) {
    toast.error(String(e));
    isIndexing.value = false;
  }
}

// Cancel face indexing
async function clickCancelIndex() {
  await cancelFaceIndex();
}

// Reset faces
async function clickResetFaces() {
  showResetFacesMsgbox.value = true;
}

async function onResetFacesConfirm() {
  showResetFacesMsgbox.value = false;
  
  // Reset selection and config
  selectedPerson.value = null;
  if (libConfig.person) {
    libConfig.person.id = null;
    libConfig.person.name = null;
  }

  await resetFaces();
  if (personSearch.value) {
    personSearch.value = '';
    await nextTick();
    if (personSearchTimer) clearTimeout(personSearchTimer);
    personSearchTimer = null;
  }
  allPersonCount.value = 0;
  await loadPersons();
  checkFaceStats();
}

async function checkFaceStats() {
  const stats = await getFaceStats();
  if (stats) {
    incompleteCount.value = stats.unprocessed;
  }
}

useLibraryChangeRefresh(() => {
  void loadPersons(true, true);
  void checkFaceStats();
});

// Face edits made outside this panel (file info, "Not <name>") change counts and may delete persons.
let unlistenPersonsChanged: (() => void) | null = null;
let personsChangedDisposed = false;
onMounted(async () => {
  const stop = await listen(PERSONS_CHANGED_EVENT, () => void loadPersons(true, true));
  if (personsChangedDisposed) stop();
  else unlistenPersonsChanged = stop;
});
onUnmounted(() => {
  personsChangedDisposed = true;
  unlistenPersonsChanged?.();
});

// Only refresh the active view. Inactive panel data is refreshed on re-entry.

watch(() => [config.main.sidebarIndex, libConfig.activePane], () => {
  if (libConfig.activePane === 'main' && config.main.sidebarIndex === SIDEBAR.PERSON) {
    void loadPersons(true, true);
    void checkFaceStats();
  }
});

async function showBetaTooltip() {
  if (!config.settings.showToolTip || !betaBadgeRef.value) return;

  isBetaTooltipVisible.value = true;
  await nextTick();

  if (!betaBadgeRef.value || !betaTooltipRef.value) return;

  const rect = betaBadgeRef.value.getBoundingClientRect();
  const tooltipRect = betaTooltipRef.value.getBoundingClientRect();
  const padding = 4;

  let top = rect.bottom + padding;
  let left = rect.left + (rect.width - tooltipRect.width) / 2;

  if (left + tooltipRect.width > window.innerWidth - padding) {
    left = window.innerWidth - tooltipRect.width - padding;
  }
  if (left < padding) {
    left = padding;
  }
  if (top + tooltipRect.height > window.innerHeight - padding) {
    top = rect.top - tooltipRect.height - padding;
  }

  betaTooltipStyle.value = {
    top: `${top}px`,
    left: `${left}px`,
  };
}

function hideBetaTooltip() {
  isBetaTooltipVisible.value = false;
}

defineExpose({
  clickIndexFaces,
  clickCancelIndex,
  loadPersons,
  clickResetFaces,
  isIndexing,
});

</script>
