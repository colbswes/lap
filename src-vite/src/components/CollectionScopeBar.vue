<template>
  <div class="mx-1 mb-1.5 px-1 shrink-0 flex items-center gap-1" data-collection-scope-bar="true">
    <div
      class="flex-1 min-w-0 grid grid-cols-3 gap-0.5 p-0.5 rounded-box bg-base-300/70 border border-base-content/5"
      role="tablist"
      :aria-label="$t('scope.label')"
    >
      <button
        v-for="item in scopeTabs"
        :key="item.id"
        type="button"
        role="tab"
        :aria-selected="activeScopeId === item.id"
        :title="item.tooltip"
        :class="[
          'h-6 px-1.5 rounded-[calc(var(--radius-box)-3px)] text-[11px] font-medium truncate transition-colors cursor-pointer flex items-center justify-center gap-1',
          activeScopeId === item.id
            ? 'bg-base-100 text-primary shadow-xs'
            : 'text-base-content/60 hover:text-base-content hover:bg-base-100/40',
        ]"
        @click="selectScope(item.id)"
      >
        <span class="truncate">{{ item.shortLabel }}</span>
        <span
          v-if="item.id !== SCOPE_EVERYTHING && item.folderCount > 0"
          class="text-[10px] tabular-nums opacity-55"
        >{{ item.folderCount }}</span>
      </button>
    </div>

    <TButton
      :icon="IconFilter"
      :buttonSize="'small'"
      :tooltip="$t('scope.configure')"
      :selected="activeScopeId !== SCOPE_EVERYTHING"
      @click="$emit('configure')"
    />
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { libConfig } from '@/common/config';
import {
  SCOPE_EVERYDAY,
  SCOPE_FAMILY,
  SCOPE_EVERYTHING,
  getScopeById,
  normalizeCollectionScopeState,
} from '@/common/collectionScopes';
import { IconFilter } from '@/common/icons';
import TButton from '@/components/TButton.vue';

const emit = defineEmits<{
  (e: 'select', scopeId: string): void;
  (e: 'configure'): void;
}>();

const { t } = useI18n();

const normalizedScope = computed(() => normalizeCollectionScopeState(libConfig.collectionScope));
const activeScopeId = computed(() => normalizedScope.value.activeId);

const scopeTabs = computed(() => {
  const everyday = getScopeById(normalizedScope.value, SCOPE_EVERYDAY);
  const family = getScopeById(normalizedScope.value, SCOPE_FAMILY);
  const everydayCount = everyday?.albumIds.length ?? 0;
  const familyCount = family?.albumIds.length ?? 0;

  return [
    {
      id: SCOPE_EVERYDAY,
      shortLabel: t('scope.everyday_short'),
      folderCount: everydayCount,
      tooltip: `${t('scope.everyday')} (${t('scope.folders_assigned', everydayCount)})`,
    },
    {
      id: SCOPE_FAMILY,
      shortLabel: t('scope.family_short'),
      folderCount: familyCount,
      tooltip: `${t('scope.family')} (${t('scope.folders_assigned', familyCount)})`,
    },
    {
      id: SCOPE_EVERYTHING,
      shortLabel: t('scope.everything_short'),
      folderCount: 0,
      tooltip: t('scope.everything'),
    },
  ];
});

function selectScope(scopeId: string) {
  emit('select', scopeId);
}
</script>
