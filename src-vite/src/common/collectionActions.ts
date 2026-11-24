import { emit as tauriEmit } from '@tauri-apps/api/event';
import { addFilesToCollection, createCollection, listCollections } from '@/common/api';
import { config, libConfig } from '@/common/config';

export function makeDefaultQueryParams(overrides: Record<string, any> = {}) {
  return {
    searchFileName: '',
    searchFileType: 0,
    favoritesOnly: false,
    sortType: 0,
    sortOrder: 1,
    randomSeed: 0,
    searchAllSubfolders: '',
    searchFolder: '',
    startDate: 0,
    endDate: 0,
    calendarSort: 0,
    make: '',
    model: '',
    lensMake: '',
    lensModel: '',
    locationAdmin1: '',
    locationName: '',
    isFavorite: false,
    tagId: 0,
    personId: 0,
    folderSort: 0,
    categorySort: 0,
    groupBy: 0,
    ...overrides,
  };
}

export function pickUniqueCollectionName(baseName: string, existingNames: string[]): string {
  const base = String(baseName || '').trim() || 'Collection';
  const taken = new Set(existingNames.map((name) => String(name || '').trim().toLocaleLowerCase()));
  if (!taken.has(base.toLocaleLowerCase())) return base;
  let suffix = 2;
  while (taken.has(`${base} ${suffix}`.toLocaleLowerCase())) {
    suffix += 1;
  }
  return `${base} ${suffix}`;
}

export async function saveFilesAsNewCollection(baseName: string, rawFileIds: number[]) {
  const fileIds = Array.from(
    new Set((rawFileIds || []).map((id) => Number(id)).filter((id) => Number.isInteger(id) && id > 0)),
  );
  const existing = (await listCollections()) || [];
  const name = pickUniqueCollectionName(
    baseName,
    existing.map((item: any) => String(item?.name || '')),
  );
  const created = await createCollection(name);
  if (!created?.id) return null;

  let addedCount = 0;
  if (fileIds.length > 0) {
    const result = await addFilesToCollection(Number(created.id), fileIds);
    addedCount = Number(result?.added ?? fileIds.length);
  }

  config.collectionTray.expanded = true;
  libConfig.activePane = 'collection';
  libConfig.collection.selectedId = Number(created.id);
  libConfig.collection.activateTick = Number(libConfig.collection.activateTick || 0) + 1;
  await tauriEmit('collections-changed', { selectedId: Number(created.id) });
  return { collection: created, addedCount };
}

export function buildSmartDraftFromTags(
  tags: Array<{ id: number; name: string }>,
  matchAny = false,
) {
  const valid = (tags || []).filter((tag) => Number(tag?.id) > 0);
  const name = valid.map((tag) => tag.name).filter(Boolean).join(matchAny ? ' / ' : ' + ');
  return {
    name,
    description: '',
    query: {
      version: 1,
      match: matchAny ? 'any' : 'all',
      rules: valid.map((tag) => ({
        field: 'tag',
        operator: 'has',
        value: Number(tag.id),
      })),
    },
  };
}

export function buildSmartDraftFromCollection(collection: { id: number; name: string }) {
  return {
    name: String(collection?.name || '').trim(),
    description: '',
    query: {
      version: 1,
      match: 'all',
      rules: [
        {
          field: 'collection',
          operator: 'is',
          value: Number(collection.id),
        },
      ],
    },
  };
}

export function buildSmartDuplicateDraft(smartAlbum: any, copySuffix = 'Copy') {
  const baseName = String(smartAlbum?.name || '').trim();
  const cloned = JSON.parse(JSON.stringify(smartAlbum || {}));
  delete cloned.id;
  delete cloned.createdAt;
  delete cloned.updatedAt;
  delete cloned.count;
  cloned.name = baseName ? `${baseName} ${copySuffix}` : copySuffix;
  return cloned;
}
