<template>

  <div class="sidebar-panel">
    <div class="sidebar-panel-header">
      <span class="sidebar-panel-header-title flex-1">{{ titlebar }}</span>
    </div>

    <div class="min-h-0 flex-1 overflow-x-hidden overflow-y-auto">
      <template v-for="item in libraryItems" :key="item.id">
      <div
        :class="[
          'sidebar-item',
          libConfig.library.item === item.id ? 'sidebar-item-selected' : 'sidebar-item-hover',
        ]"
        @click="selectItem(item.id)"
      >
        <component :is="item.icon" class="mx-1 w-5 h-5 shrink-0" />
        <div class="sidebar-item-label">
          <span>{{ item.label }}</span>
        </div>
        <div class="ml-auto flex items-center">
          <span v-if="item.count && item.count > 0" class="sidebar-item-count">
            {{ item.count.toLocaleString() }}
          </span>
        </div>
      </div>
      </template>

      <div class="sidebar-item sidebar-item-hover" @click="toggleSubjects">
        <IconRight
          :class="[
            'p-1 w-6 h-6 shrink-0 transition-transform',
            libConfig.library.subjectsExpanded ? 'rotate-90' : '',
          ]"
          @click.stop="toggleSubjects"
        />
        <span class="sidebar-item-label">
          {{ localeMsg.subject.title }}
        </span>
      </div>

      <Transition
        @before-enter="onBeforeEnter"
        @enter="onEnter"
        @after-enter="onAfterEnter"
        @leave="onLeave"
      >
        <div v-if="libConfig.library.subjectsExpanded" class="overflow-hidden">
          <ul class="mb-2">
            <li v-for="item in smartTagItems" :key="item.id" class="pl-4">
              <div
                :class="[
                  'sidebar-item sidebar-item-compact ml-2',
                  libConfig.library.item === LIB_ITEM.SUBJECTS && libConfig.library.smartId === item.id ? 'sidebar-item-selected' : 'sidebar-item-hover',
                ]"
                @click="selectSmartTag(item.id)"
              >
                <IconBolt class="mx-1 w-4 h-4 shrink-0" />
                <span class="sidebar-item-label">{{ item.label }}</span>
                <span v-if="item.count" class="text-[10px] tabular-nums text-base-content/30 mr-2">{{ formatSearchResultCount(item.count) }}</span>
              </div>
            </li>
          </ul>
        </div>
      </Transition>
    </div>

  </div>

</template>

<script setup lang="ts">
import { computed, onMounted, watch } from 'vue';
import { useI18n } from 'vue-i18n';
import { config, libConfig } from '@/common/config';
import { useUIStore } from '@/stores/uiStore';
import { createEmptyLibraryCounts } from '@/stores/libraryStore';
import { LIB_ITEM, SIDEBAR, type LibItem } from '@/common/constants';

import { IconFiles, IconHeartFilled, IconRight, IconBolt, IconHistory } from '@/common/icons';
import { SMART_TAG_CATEGORIES } from '@/common/smartTags';
import { getLibraryVisibleCounts } from '@/common/api';
import { useLibraryChangeRefresh } from '@/composables/useLibraryChangeRefresh';

const props = defineProps({
  titlebar: {
    type: String,
    required: true
  }
});

const { locale, messages } = useI18n();
const localeMsg = computed(() => messages.value[locale.value] as any);
const uiStore = useUIStore();
const libraryCounts = computed(() => libConfig.library.counts || createEmptyLibraryCounts());
const subjectCounts = computed(() => libConfig.library.subjectCounts || {});
const totalCount = computed(() => Number(libraryCounts.value.all || 0));
const favoriteCount = computed(() => Number(libraryCounts.value.favorite || 0));
const todayCount = computed(() => Number(libraryCounts.value.today || 0));
let libraryCountRequest = 0;
async function refreshLibraryCounts() {
  const request = ++libraryCountRequest;
  const libraryId = libConfig._libraryId;
  const counts = await getLibraryVisibleCounts();
  if (
    !counts
    || request !== libraryCountRequest
    || libraryId !== libConfig._libraryId
  ) return;
  libConfig.library.counts = {
    all: counts.all, favorite: counts.favorite, today: counts.today,
  };
}
onMounted(() => { void refreshLibraryCounts(); });
useLibraryChangeRefresh(refreshLibraryCounts);
const isActiveLibraryView = () => libConfig.activePane === 'main' && config.main.sidebarIndex === SIDEBAR.LIBRARY;
watch(() => [config.main.sidebarIndex, libConfig.activePane], () => {
  if (isActiveLibraryView()) void refreshLibraryCounts();
});
const libraryItems = computed(() => [
  {
    id: LIB_ITEM.ALL,
    label: localeMsg.value.library.all_files,
    icon: IconFiles,
    count: totalCount.value,
  },
  {
    id: LIB_ITEM.TODAY,
    label: localeMsg.value.library.on_this_day,
    icon: IconHistory,
    count: todayCount.value,
  },
  {
    id: LIB_ITEM.FAV,
    label: localeMsg.value.favorite.files,
    icon: IconHeartFilled,
    count: favoriteCount.value,
  },
]);

const smartTagItems = computed(() =>
  SMART_TAG_CATEGORIES.map(category => {
    const item = category.items[0];
    return {
      id: item.id,
      label: localeMsg.value.subject.items?.[item.id] || item.id,
      count: Number(subjectCounts.value[item.id] || 0),
    };
  })
);

function formatSearchResultCount(count: number) {
  return count.toLocaleString();
}

function selectItem(item: LibItem) {
  libConfig.library.item = item;
  uiStore.requestCountUpdate({ source: 'library', item });
  libConfig.library.activateTick = Number(libConfig.library.activateTick || 0) + 1;
}

function toggleSubjects() {
  libConfig.library.subjectsExpanded = !libConfig.library.subjectsExpanded;
}

function onBeforeEnter(el: Element) {
  const element = el as HTMLElement;
  element.style.opacity = '0';
  element.style.height = '0';
}

function onEnter(el: Element) {
  const element = el as HTMLElement;
  element.style.transition = 'all 0.1s ease';
  element.style.height = `${element.scrollHeight}px`;
  element.style.opacity = '1';
}

function onAfterEnter(el: Element) {
  (el as HTMLElement).style.height = '';
}

function onLeave(el: Element) {
  const element = el as HTMLElement;
  element.style.transition = 'all 0.1s ease';
  element.style.height = `${element.scrollHeight}px`;
  void element.offsetHeight;
  element.style.height = '0';
  element.style.opacity = '0';
}

function selectSmartTag(smartId: string) {
  libConfig.library.item = LIB_ITEM.SUBJECTS;
  libConfig.library.smartId = smartId;
  uiStore.requestCountUpdate({ source: 'library', item: LIB_ITEM.SUBJECTS, smartId });
  libConfig.library.activateTick = Number(libConfig.library.activateTick || 0) + 1;
}

</script>
