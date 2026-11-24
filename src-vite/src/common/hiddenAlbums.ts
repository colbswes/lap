export function normalizeHiddenIds(raw: unknown): number[] {
  if (!Array.isArray(raw)) return [];
  const unique = new Set<number>();
  for (const value of raw) {
    const id = Number(value);
    if (Number.isInteger(id) && id > 0) unique.add(id);
  }
  return Array.from(unique).sort((a, b) => a - b);
}

export function normalizeHiddenStringIds(raw: unknown): string[] {
  if (!Array.isArray(raw)) return [];
  const unique = new Set<string>();
  for (const value of raw) {
    const id = String(value ?? '').trim();
    if (id) unique.add(id);
  }
  return Array.from(unique).sort();
}

export function isHiddenId(hiddenIds: unknown, targetId: number): boolean {
  const id = Number(targetId);
  if (!Number.isInteger(id) || id <= 0) return false;
  return normalizeHiddenIds(hiddenIds).includes(id);
}

export function isHiddenStringId(hiddenIds: unknown, targetId: string): boolean {
  const id = String(targetId ?? '').trim();
  if (!id) return false;
  return normalizeHiddenStringIds(hiddenIds).includes(id);
}

export function withToggledHiddenId(hiddenIds: unknown, targetId: number): number[] {
  const id = Number(targetId);
  const normalized = normalizeHiddenIds(hiddenIds);
  if (!Number.isInteger(id) || id <= 0) return normalized;
  if (normalized.includes(id)) {
    return normalized.filter((item) => item !== id);
  }
  return normalizeHiddenIds([...normalized, id]);
}

export function withToggledHiddenStringId(hiddenIds: unknown, targetId: string): string[] {
  const id = String(targetId ?? '').trim();
  const normalized = normalizeHiddenStringIds(hiddenIds);
  if (!id) return normalized;
  if (normalized.includes(id)) {
    return normalized.filter((item) => item !== id);
  }
  return normalizeHiddenStringIds([...normalized, id]);
}

export function withoutHiddenId(hiddenIds: unknown, targetId: number): number[] {
  const id = Number(targetId);
  const normalized = normalizeHiddenIds(hiddenIds);
  if (!Number.isInteger(id) || id <= 0) return normalized;
  return normalized.filter((item) => item !== id);
}

export function withHiddenIds(hiddenIds: unknown, targetIds: unknown): number[] {
  const normalized = normalizeHiddenIds(hiddenIds);
  const additions = normalizeHiddenIds(targetIds);
  if (additions.length === 0) return normalized;
  return normalizeHiddenIds([...normalized, ...additions]);
}

export function withoutHiddenIds(hiddenIds: unknown, targetIds: unknown): number[] {
  const normalized = normalizeHiddenIds(hiddenIds);
  const removals = new Set(normalizeHiddenIds(targetIds));
  if (removals.size === 0) return normalized;
  return normalized.filter((item) => !removals.has(item));
}

export function withoutHiddenStringId(hiddenIds: unknown, targetId: string): string[] {
  const id = String(targetId ?? '').trim();
  const normalized = normalizeHiddenStringIds(hiddenIds);
  if (!id) return normalized;
  return normalized.filter((item) => item !== id);
}

export const normalizeHiddenAlbumIds = normalizeHiddenIds;
export const isAlbumHidden = isHiddenId;
export const withToggledHiddenAlbum = withToggledHiddenId;
export const withoutHiddenAlbum = withoutHiddenId;
