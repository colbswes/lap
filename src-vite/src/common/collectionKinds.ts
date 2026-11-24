import { IconCollection, IconFestival, IconMusic } from '@/common/icons';

/** Event type of a collection; `null` is a plain collection. */
export type CollectionKind = 'concert' | 'festival' | 'other' | null;

export const COLLECTION_KINDS: CollectionKind[] = [null, 'concert', 'festival', 'other'];

export function collectionKindIcon(kind: string | null | undefined) {
  if (kind === 'concert') return IconMusic;
  if (kind === 'festival') return IconFestival;
  return IconCollection;
}
