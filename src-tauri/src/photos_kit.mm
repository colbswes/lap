#import <Foundation/Foundation.h>
#import <Photos/Photos.h>

extern "C" {

static void lap_set_error(char **error_out, NSString *message) {
    if (!error_out || !message) return;
    *error_out = strdup([message UTF8String]);
}

/// Delete Apple Photos assets by their bare UUID strings (moves them to Recently Deleted).
/// Returns >= 0 for number of assets deleted, or -1 on authorization/runtime error.
int lap_delete_apple_photos_assets(const char *uuids_json, char **error_out) {
    @autoreleasepool {
        if (!uuids_json) return 0;

        NSData *jsonData = [[NSString stringWithUTF8String:uuids_json]
            dataUsingEncoding:NSUTF8StringEncoding];
        if (!jsonData) return 0;

        id parsed = [NSJSONSerialization JSONObjectWithData:jsonData options:0 error:nil];
        if (![parsed isKindOfClass:[NSArray class]]) return 0;

        NSArray *rawUuids = (NSArray *)parsed;
        if (rawUuids.count == 0) return 0;

        NSMutableSet<NSString *> *wanted = [NSMutableSet setWithCapacity:rawUuids.count];
        NSMutableArray<NSString *> *candidates = [NSMutableArray arrayWithCapacity:rawUuids.count * 2];
        for (id item in rawUuids) {
            if (![item isKindOfClass:[NSString class]]) continue;
            NSString *uuid = [[(NSString *)item stringByTrimmingCharactersInSet:
                [NSCharacterSet whitespaceAndNewlineCharacterSet]] uppercaseString];
            if (uuid.length == 0) continue;
            [wanted addObject:uuid];
            [candidates addObject:[NSString stringWithFormat:@"%@/L0/001", uuid]];
            [candidates addObject:uuid];
        }
        if (wanted.count == 0) return 0;

        __block PHAuthorizationStatus status =
            [PHPhotoLibrary authorizationStatusForAccessLevel:PHAccessLevelReadWrite];
        if (status == PHAuthorizationStatusNotDetermined) {
            dispatch_semaphore_t authSem = dispatch_semaphore_create(0);
            [PHPhotoLibrary requestAuthorizationForAccessLevel:PHAccessLevelReadWrite
                                                       handler:^(PHAuthorizationStatus newStatus) {
                status = newStatus;
                dispatch_semaphore_signal(authSem);
            }];
            dispatch_semaphore_wait(authSem, DISPATCH_TIME_FOREVER);
        }

        if (status != PHAuthorizationStatusAuthorized && status != PHAuthorizationStatusLimited) {
            lap_set_error(
                error_out,
                @"Photos library access denied. Grant Lap Photos permission in System Settings > Privacy & Security > Photos."
            );
            return -1;
        }

        PHFetchOptions *options = [[PHFetchOptions alloc] init];
        options.includeHiddenAssets = YES;
        options.includeAllBurstAssets = YES;

        NSMutableArray<PHAsset *> *matched = [NSMutableArray arrayWithCapacity:wanted.count];
        NSMutableSet<NSString *> *foundPrefixes = [NSMutableSet setWithCapacity:wanted.count];

        PHFetchResult<PHAsset *> *direct =
            [PHAsset fetchAssetsWithLocalIdentifiers:candidates options:options];
        for (PHAsset *asset in direct) {
            [matched addObject:asset];
            NSString *prefix = [[[asset.localIdentifier componentsSeparatedByString:@"/"] firstObject] uppercaseString];
            if (prefix) [foundPrefixes addObject:prefix];
        }

        if (foundPrefixes.count < wanted.count) {
            NSMutableSet<NSString *> *remaining = [wanted mutableCopy];
            [remaining minusSet:foundPrefixes];
            PHFetchResult<PHAsset *> *allAssets = [PHAsset fetchAssetsWithOptions:options];
            for (PHAsset *asset in allAssets) {
                NSString *prefix = [[[asset.localIdentifier componentsSeparatedByString:@"/"] firstObject] uppercaseString];
                if (prefix && [remaining containsObject:prefix]) {
                    [matched addObject:asset];
                    [remaining removeObject:prefix];
                    if (remaining.count == 0) break;
                }
            }
        }

        if (matched.count == 0) {
            return 0;
        }

        __block BOOL deleteOk = NO;
        __block NSError *deleteErr = nil;
        dispatch_semaphore_t delSem = dispatch_semaphore_create(0);

        [[PHPhotoLibrary sharedPhotoLibrary] performChanges:^{
            [PHAssetChangeRequest deleteAssets:matched];
        } completionHandler:^(BOOL success, NSError * _Nullable error) {
            deleteOk = success;
            deleteErr = error;
            dispatch_semaphore_signal(delSem);
        }];
        dispatch_semaphore_wait(delSem, DISPATCH_TIME_FOREVER);

        if (!deleteOk) {
            NSString *msg = deleteErr
                ? deleteErr.localizedDescription
                : @"Apple Photos deletion was cancelled or failed.";
            lap_set_error(error_out, msg);
            return -1;
        }

        return (int)matched.count;
    }
}

}
