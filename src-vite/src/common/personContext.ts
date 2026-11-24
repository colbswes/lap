import { ref } from 'vue';

/** Person whose photos the content grid is currently showing (null otherwise). */
export const personViewContext = ref<{ id: number; name: string } | null>(null);

/** Tauri event emitted after face/person assignments change outside the People panel. */
export const PERSONS_CHANGED_EVENT = 'persons-changed';
