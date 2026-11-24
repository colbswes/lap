import type { RawPreviewSource } from '@/common/rawDisplay';

export type PrefetchFile = {
  id?: number;
  file_path: string;
  file_type: number;
  modified_at?: number;
  media_subtype?: string;
  live_photo_video_path?: string;
};

export type LoadedImage = {
  src: string;
  naturalWidth: number;
  naturalHeight: number;
  decoded?: HTMLImageElement;
  raw?: { source: RawPreviewSource; unavailable: boolean; pair: string | null };
};

export function getImageNeighbors(files: Array<PrefetchFile & { isPlaceholder?: boolean }>, index: number, direction: number): PrefetchFile[] {
  if (index < 0 || index >= files.length) return [];
  const step = direction < 0 ? -1 : 1;
  return [step, 2 * step, -step, -2 * step].flatMap(offset => {
    const file = files[index + offset];
    if (!file || file.isPlaceholder || !file.file_path || ![1, 3].includes(file.file_type)) return [];
    const { id, file_path, file_type, modified_at, media_subtype, live_photo_video_path } = file;
    return [{ id, file_path, file_type, modified_at, media_subtype, live_photo_video_path }];
  });
}

const cacheLimit = 12;
const cache = new Map<string, { filePath: string; image: LoadedImage }>();
const cachedRawUrls = new Set<string>();
const displayedUrls = new Map<string, number>();

export function releaseRawUrl(src: string) {
  if (src.startsWith('blob:') && !cachedRawUrls.has(src) && !displayedUrls.has(src)) URL.revokeObjectURL(src);
}

export function retainImageSlot(src: string) {
  if (src.startsWith('blob:')) displayedUrls.set(src, (displayedUrls.get(src) || 0) + 1);
}

export function releaseImageSlot(src: string) {
  const count = displayedUrls.get(src);
  if (!count) return;
  if (count > 1) displayedUrls.set(src, count - 1);
  else displayedUrls.delete(src);
  releaseRawUrl(src);
}

function evict(key: string) {
  const entry = cache.get(key);
  if (!entry) return;
  cache.delete(key);
  if (entry.image.raw) {
    cachedRawUrls.delete(entry.image.src);
    releaseRawUrl(entry.image.src);
  }
}

export function getDecodedImage(key: string) {
  const entry = cache.get(key);
  if (!entry) return undefined;
  cache.delete(key);
  cache.set(key, entry);
  return entry.image;
}

export function cacheDecodedImage(key: string, filePath: string, image: LoadedImage) {
  evict(key);
  if (image.raw) cachedRawUrls.add(image.src);
  cache.set(key, { filePath, image });
  if (cache.size > cacheLimit) evict(cache.keys().next().value!);
}

export function clearDecodedImages(filePath?: string) {
  for (const [key, entry] of cache) {
    if (!filePath || entry.filePath === filePath) evict(key);
  }
}

const prefetchQueue: Array<() => void> = [];
let activePrefetches = 0;

function drainPrefetchQueue() {
  while (activePrefetches < 2 && prefetchQueue.length) prefetchQueue.shift()!();
}

export function queueImagePrefetch(signal: AbortSignal, load: () => Promise<void>): Promise<void> {
  if (signal.aborted) return Promise.resolve();
  return new Promise(resolve => {
    const cancel = () => {
      const index = prefetchQueue.indexOf(start);
      if (index >= 0) prefetchQueue.splice(index, 1);
      resolve();
    };
    const start = () => {
      signal.removeEventListener('abort', cancel);
      activePrefetches++;
      void Promise.resolve().then(() => {
        if (!signal.aborted) return load();
      }).catch(() => {
        // Active viewing handles and reports load failures.
      }).finally(() => {
        activePrefetches--;
        resolve();
        drainPrefetchQueue();
      });
    };
    signal.addEventListener('abort', cancel, { once: true });
    prefetchQueue.push(start);
    drainPrefetchQueue();
  });
}
