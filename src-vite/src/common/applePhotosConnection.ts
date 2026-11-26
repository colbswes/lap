import { reactive } from 'vue';
import { invoke } from '@tauri-apps/api/core';
import { emit as tauriEmit } from '@tauri-apps/api/event';
import { getAllAlbums } from '@/common/api';

/// The Apple Photos connection of the current library. PhotoKit only exposes
/// the System Photo Library, so a library has at most one connected album.
export const applePhotosConnection = reactive({
  albumId: 0,
  busy: false,
});

export async function refreshApplePhotosConnection(): Promise<number> {
  try {
    const status: any = await invoke('photos_source_status');
    applePhotosConnection.albumId = Number(status?.albumId || 0);
  } catch (error) {
    console.error('Failed to read the Apple Photos connection:', error);
  }
  return applePhotosConnection.albumId;
}

async function refreshLibraryViews() {
  await tauriEmit('albums-refreshed', { albums: await getAllAlbums(), refreshFolders: true });
  await tauriEmit('library-total-refreshed');
}

/// Adds the Apple Photos library as a read-only folder and catalogs every item.
export async function connectApplePhotos(albumName: string) {
  if (applePhotosConnection.busy) return null;
  applePhotosConnection.busy = true;
  try {
    const report: any = await invoke('photos_source_connect', { name: albumName });
    applePhotosConnection.albumId = Number(report?.albumId || 0);
    await refreshLibraryViews();
    return report;
  } finally {
    applePhotosConnection.busy = false;
  }
}

let lastSync = 0;
let lastRevision = -1;
/// Picks up photos added, edited or deleted in Apple Photos. Unless `force` is
/// set, it only runs when Photos reported a change since the last sync.
export async function syncApplePhotos(minIntervalMs = 0, force = false) {
  if (!applePhotosConnection.albumId || applePhotosConnection.busy) return null;
  if (Date.now() - lastSync < minIntervalMs) return null;
  const revision = Number(await invoke('photos_source_revision').catch(() => -1));
  if (!force && revision >= 0 && revision === lastRevision) return null;
  lastSync = Date.now();
  applePhotosConnection.busy = true;
  try {
    const report: any = await invoke('photos_source_sync');
    lastRevision = revision;
    if (report && report.added + report.removed > 0) {
      await tauriEmit('albums-refreshed', { albums: await getAllAlbums(), refreshFolders: true });
    }
    return report;
  } catch (error) {
    console.error('Apple Photos sync failed:', error);
    return null;
  } finally {
    applePhotosConnection.busy = false;
  }
}
