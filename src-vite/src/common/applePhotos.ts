/// Placeholder root for items Lap reads from Apple Photos (see t_photos_source.rs).
export const APPLE_PHOTOS_ROOT = '/.lap-apple-photos';

export function isApplePhotosPath(filePath = ''): boolean {
  return filePath === APPLE_PHOTOS_ROOT || String(filePath).startsWith(`${APPLE_PHOTOS_ROOT}/`);
}
