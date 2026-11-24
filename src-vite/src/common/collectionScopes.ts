export const SCOPE_EVERYTHING = 'everything';
export const SCOPE_EVERYDAY = 'everyday';
export const SCOPE_FAMILY = 'family';

export interface CollectionScopeItem {
  id: string;
  name: string;
  albumIds: number[];
}

export interface CollectionScopeState {
  activeId: string;
  scopes: CollectionScopeItem[];
}

export function createDefaultCollectionScopes(): CollectionScopeItem[] {
  return [
    { id: SCOPE_EVERYDAY, name: 'Everyday', albumIds: [] },
    { id: SCOPE_FAMILY, name: 'Family Archive', albumIds: [] },
  ];
}

export function createDefaultCollectionScopeState(): CollectionScopeState {
  return {
    activeId: SCOPE_EVERYTHING,
    scopes: createDefaultCollectionScopes(),
  };
}

function normalizeAlbumIds(ids: unknown): number[] {
  if (!Array.isArray(ids)) return [];
  const unique = new Set<number>();
  for (const raw of ids) {
    const id = Number(raw);
    if (Number.isInteger(id) && id > 0) unique.add(id);
  }
  return Array.from(unique).sort((a, b) => a - b);
}

export function normalizeCollectionScopeState(raw: any): CollectionScopeState {
  const incomingScopes = Array.isArray(raw?.scopes) ? raw.scopes : [];
  const byId = new Map<string, CollectionScopeItem>();
  const extraOrder: string[] = [];

  for (const entry of incomingScopes) {
    const id = String(entry?.id || '').trim();
    if (!id || id === SCOPE_EVERYTHING) continue;
    const item: CollectionScopeItem = {
      id,
      name: String(entry?.name || '').trim(),
      albumIds: normalizeAlbumIds(entry?.albumIds ?? entry?.album_ids),
    };
    if (!byId.has(id) && id !== SCOPE_EVERYDAY && id !== SCOPE_FAMILY) {
      extraOrder.push(id);
    }
    byId.set(id, item);
  }

  const scopes: CollectionScopeItem[] = [];
  for (const defaultItem of createDefaultCollectionScopes()) {
    const existing = byId.get(defaultItem.id);
    if (existing) {
      scopes.push({
        id: defaultItem.id,
        name: existing.name || defaultItem.name,
        albumIds: existing.albumIds,
      });
      byId.delete(defaultItem.id);
    } else {
      scopes.push(defaultItem);
    }
  }
  for (const id of extraOrder) {
    const item = byId.get(id);
    if (item) scopes.push(item);
  }

  const rawActiveId = String(raw?.activeId ?? raw?.active_id ?? SCOPE_EVERYTHING).trim();
  const activeId =
    rawActiveId === SCOPE_EVERYTHING || scopes.some((scope) => scope.id === rawActiveId)
      ? rawActiveId
      : SCOPE_EVERYTHING;

  return { activeId, scopes };
}

export function getScopeById(
  state: CollectionScopeState | null | undefined,
  scopeId: string
): CollectionScopeItem | null {
  if (!state || scopeId === SCOPE_EVERYTHING) return null;
  return state.scopes.find((scope) => scope.id === scopeId) || null;
}

/**
 * Returns `null` for "everything" (all albums visible), or a sorted array of
 * album IDs for the active named scope.
 */
export function getActiveScopeAlbumIds(
  state: CollectionScopeState | null | undefined
): number[] | null {
  const normalized = normalizeCollectionScopeState(state);
  if (normalized.activeId === SCOPE_EVERYTHING) return null;
  const scope = getScopeById(normalized, normalized.activeId);
  return scope ? [...scope.albumIds] : [];
}

export function isAlbumInScope(
  state: CollectionScopeState | null | undefined,
  scopeId: string,
  albumId: number
): boolean {
  const id = Number(albumId);
  if (!Number.isInteger(id) || id <= 0) return false;
  if (scopeId === SCOPE_EVERYTHING) return true;
  const scope = getScopeById(normalizeCollectionScopeState(state), scopeId);
  return !!scope && scope.albumIds.includes(id);
}

export function isActiveScopeUnassigned(
  state: CollectionScopeState | null | undefined,
  existingAlbumIds?: number[]
): boolean {
  const activeIds = getActiveScopeAlbumIds(state);
  if (activeIds === null) return false;
  if (activeIds.length === 0) return true;
  if (Array.isArray(existingAlbumIds)) {
    const existing = new Set(existingAlbumIds.map((id) => Number(id)));
    return !activeIds.some((id) => existing.has(id));
  }
  return false;
}

export function withActiveScope(
  state: CollectionScopeState | null | undefined,
  scopeId: string
): CollectionScopeState {
  const normalized = normalizeCollectionScopeState(state);
  return normalizeCollectionScopeState({
    ...normalized,
    activeId: scopeId,
  });
}

export function withScopeAlbums(
  state: CollectionScopeState | null | undefined,
  scopeId: string,
  albumIds: number[]
): CollectionScopeState {
  const normalized = normalizeCollectionScopeState(state);
  if (scopeId === SCOPE_EVERYTHING) return normalized;
  return normalizeCollectionScopeState({
    ...normalized,
    scopes: normalized.scopes.map((scope) =>
      scope.id === scopeId ? { ...scope, albumIds: normalizeAlbumIds(albumIds) } : scope
    ),
  });
}

export function withToggledAlbumInScope(
  state: CollectionScopeState | null | undefined,
  scopeId: string,
  albumId: number
): CollectionScopeState {
  const normalized = normalizeCollectionScopeState(state);
  const id = Number(albumId);
  if (scopeId === SCOPE_EVERYTHING || !Number.isInteger(id) || id <= 0) return normalized;
  const scope = getScopeById(normalized, scopeId);
  if (!scope) return normalized;
  const nextIds = scope.albumIds.includes(id)
    ? scope.albumIds.filter((item) => item !== id)
    : [...scope.albumIds, id];
  return withScopeAlbums(normalized, scopeId, nextIds);
}

export function pruneRemovedAlbumFromScopes(
  state: CollectionScopeState | null | undefined,
  albumId: number
): CollectionScopeState {
  const normalized = normalizeCollectionScopeState(state);
  const id = Number(albumId);
  if (!Number.isInteger(id) || id <= 0) return normalized;
  return normalizeCollectionScopeState({
    ...normalized,
    scopes: normalized.scopes.map((scope) => ({
      ...scope,
      albumIds: scope.albumIds.filter((item) => item !== id),
    })),
  });
}
