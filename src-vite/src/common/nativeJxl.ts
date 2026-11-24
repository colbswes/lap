// Failed original URLs include the source path and file versions. Keep only a
// bounded, per-WebView-session record; never disable JXL globally for one file.
import type { LoadedImage } from '@/common/viewerImages';

const failedOriginals = new Set<string>();

export async function loadNativeJxl(
  original: string,
  fallback: () => string,
  load: (src: string) => Promise<LoadedImage>,
  signal?: AbortSignal,
) {
  if (original && !failedOriginals.has(original)) {
    try { return await load(original); }
    catch {
      signal?.throwIfAborted();
      failedOriginals.add(original);
      if (failedOriginals.size > 256) failedOriginals.delete(failedOriginals.values().next().value!);
    }
  }
  return load(fallback());
}

export function loadBrowserImage(src: string, signal?: AbortSignal, requireDecode = true): Promise<LoadedImage> {
  return new Promise((resolve, reject) => {
    signal?.throwIfAborted();
    if (!src) { reject(new Error('Missing image source')); return; }
    const img = new Image();
    const cleanup = () => {
      signal?.removeEventListener('abort', abort);
      img.onload = null;
      img.onerror = null;
    };
    const abort = () => {
      cleanup();
      img.removeAttribute('src');
      reject(signal?.reason);
    };
    signal?.addEventListener('abort', abort, { once: true });
    img.decoding = 'async';
    img.onerror = () => {
      cleanup();
      reject(new Error('Image load failed'));
    };
    img.onload = async () => {
      try {
        try { await img.decode(); } catch (error) { if (requireDecode) throw error; }
        signal?.throwIfAborted();
        if (!img.naturalWidth || !img.naturalHeight) throw new Error('Empty image');
        const loaded: LoadedImage = { src, naturalWidth: img.naturalWidth, naturalHeight: img.naturalHeight };
        if (signal) loaded.decoded = img;
        resolve(loaded);
      } catch (error) {
        reject(error);
      } finally {
        cleanup();
      }
    };
    img.src = src;
  });
}
