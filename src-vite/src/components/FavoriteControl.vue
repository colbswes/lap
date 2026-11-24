<template>
  <div class="h-6 flex items-center gap-0.5">
    <button
      class="btn btn-ghost btn-xs min-h-0 h-6 w-6 p-0 mr-1 relative"
      :title="withShortcut(favoriteTitle, 'meta.favorite')"
      :aria-label="withMixedState(favoriteTitle, favorite === null)"
      :disabled="disabled"
      @click.stop="emit('favorite', favorite === true ? false : true)"
    >
      <component
        :is="favorite === true ? IconHeartFilled : IconHeart"
        class="w-3.5 h-3.5"
        :class="favoriteIconClass"
      />
      <IconHeartFilled v-if="favorite === null" aria-hidden="true"
        class="absolute w-3.5 h-3.5 [clip-path:inset(0_50%_0_0)]"
        :class="disabled ? 'text-base-content/30' : 'text-error'" />
    </button>
  </div>
</template>

<script setup lang="ts">
import { computed } from 'vue';
import { useI18n } from 'vue-i18n';
import { getShortcutLabel, type ShortcutActionId } from '@/common/shortcuts';
import { IconHeart, IconHeartFilled } from '@/common/icons';

const props = withDefaults(defineProps<{
  favorite: boolean | null;
  disabled?: boolean;
  favoriteLabel?: string;
  unfavoriteLabel?: string;
}>(), {
  disabled: false,
  favoriteLabel: '',
  unfavoriteLabel: '',
});

const emit = defineEmits<{
  favorite: [value: boolean];
}>();

const { t } = useI18n();

const favoriteTitle = computed(() => {
  if (props.favorite) {
    return props.unfavoriteLabel || t('menu.meta.unfavorite');
  }
  return props.favoriteLabel || t('menu.meta.favorite');
});

const favoriteIconClass = computed(() => {
  if (props.disabled) return 'text-base-content/30';
  if (props.favorite === true) return 'text-error';
  return 'text-base-content/70 hover:text-error/70';
});

function withShortcut(label: string, action: ShortcutActionId) {
  const shortcut = !props.disabled && getShortcutLabel(action);
  return shortcut ? `${label} (${shortcut})` : label;
}

function withMixedState(label: string, mixed: boolean) {
  return mixed ? `${label} (${t('favorite.mixed')})` : label;
}
</script>
