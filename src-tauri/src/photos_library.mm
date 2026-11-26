#import <AppKit/AppKit.h>
#import <AVFoundation/AVFoundation.h>
#import <Photos/Photos.h>
#include <atomic>

// Counts PhotoKit change notifications so Lap can skip syncs when nothing changed.
static std::atomic<uint64_t> libraryRevision{0};

@interface LapPhotosObserver : NSObject <PHPhotoLibraryChangeObserver>
@end
@implementation LapPhotosObserver
- (void)photoLibraryDidChange:(__unused PHChange *)change {
    ++libraryRevision;
}
@end

static BOOL authorize(NSString **error) {
    PHAuthorizationStatus status = [PHPhotoLibrary authorizationStatusForAccessLevel:PHAccessLevelReadWrite];
    if (status == PHAuthorizationStatusNotDetermined) {
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        __block PHAuthorizationStatus response;
        [PHPhotoLibrary requestAuthorizationForAccessLevel:PHAccessLevelReadWrite handler:^(PHAuthorizationStatus result) {
            response = result;
            dispatch_semaphore_signal(done);
        }];
        dispatch_semaphore_wait(done, DISPATCH_TIME_FOREVER);
        status = response;
    }
    if (status == PHAuthorizationStatusAuthorized || status == PHAuthorizationStatusLimited) return YES;
    *error = @"Allow Lap to access Photos in System Settings > Privacy & Security > Photos.";
    return NO;
}

// ---------------------------------------------------------------------------
// Apple Photos as a library source: binary image and media-path calls used by
// Lap's thumbnail, preview and media routes. All calls block; never call them
// on the main thread.

#import <ImageIO/ImageIO.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

static PHAsset *sourceAsset(const char *identifier) {
    NSString *localId = [NSString stringWithUTF8String:identifier];
    if (!localId.length) return nil;
    PHFetchOptions *options = [[PHFetchOptions alloc] init];
    options.includeAllBurstAssets = YES;
    options.includeHiddenAssets = YES;
    // Shared Album items live outside the user library source.
    options.includeAssetSourceTypes = PHAssetSourceTypeUserLibrary | PHAssetSourceTypeCloudShared | PHAssetSourceTypeiTunesSynced;
    return [PHAsset fetchAssetsWithLocalIdentifiers:@[localId] options:options].firstObject;
}

static void setError(char **errorOut, NSString *message) {
    if (errorOut) *errorOut = strdup((message ?: @"Apple Photos request failed.").UTF8String);
}

static PHCachingImageManager *sourceImageManager() {
    static PHCachingImageManager *manager;
    static dispatch_once_t once;
    dispatch_once(&once, ^{ manager = [[PHCachingImageManager alloc] init]; });
    return manager;
}

static NSData *encodeJPEG(CGImageRef pixels, double quality) {
    NSMutableData *output = [NSMutableData data];
    CGImageDestinationRef destination = CGImageDestinationCreateWithData((__bridge CFMutableDataRef)output,
        (__bridge CFStringRef)UTTypeJPEG.identifier, 1, NULL);
    if (!destination) return nil;
    CGImageDestinationAddImage(destination, pixels, (__bridge CFDictionaryRef)@{
        (__bridge NSString *)kCGImageDestinationLossyCompressionQuality: @(quality)});
    BOOL ok = CGImageDestinationFinalize(destination);
    CFRelease(destination);
    return ok ? output : nil;
}

/// Renders the asset's current version as an upright JPEG whose longer edge is
/// at most `size` pixels. `network` allows downloading iCloud originals.
/// Returns malloc'd bytes (free with lap_free_string) or NULL with *errorOut set.
/// *errorOut stays NULL when the item needs an iCloud download that was not allowed.
extern "C" uint8_t *lap_photos_image(const char *identifier, uint32_t size, bool network, size_t *length, char **errorOut) {
    @autoreleasepool {
        *length = 0;
        NSString *error = nil;
        if (!authorize(&error)) { setError(errorOut, error); return NULL; }
        PHAsset *asset = sourceAsset(identifier);
        if (!asset) { setError(errorOut, @"This item is no longer in Apple Photos."); return NULL; }
        PHImageRequestOptions *options = [[PHImageRequestOptions alloc] init];
        options.synchronous = YES;
        options.networkAccessAllowed = network;
        options.deliveryMode = PHImageRequestOptionsDeliveryModeHighQualityFormat;
        options.resizeMode = PHImageRequestOptionsResizeModeFast;
        options.version = PHImageRequestOptionsVersionCurrent;
        CGFloat edge = MAX(size, 64u);
        CGFloat scale = MIN(1.0, edge / (CGFloat)MAX(MAX(asset.pixelWidth, asset.pixelHeight), (NSUInteger)1));
        CGSize target = asset.pixelWidth && asset.pixelHeight
            ? CGSizeMake(MAX(1.0, round(asset.pixelWidth * scale)), MAX(1.0, round(asset.pixelHeight * scale)))
            : CGSizeMake(edge, edge);
        __block NSImage *result;
        __block NSDictionary *details;
        [sourceImageManager() requestImageForAsset:asset targetSize:target contentMode:PHImageContentModeAspectFit
            options:options resultHandler:^(NSImage *value, NSDictionary *info) {
            result = value;
            details = info;
        }];
        if (!result) {
            if (!network && [details[PHImageResultIsInCloudKey] boolValue]) return NULL;
            setError(errorOut, [details[PHImageErrorKey] localizedDescription] ?: @"Photos could not render this item.");
            return NULL;
        }
        CGImageRef pixels = [result CGImageForProposedRect:NULL context:nil hints:nil];
        NSData *jpeg = pixels ? encodeJPEG(pixels, size <= 1024 ? 0.82 : 0.9) : nil;
        if (!jpeg.length) { setError(errorOut, @"Photos returned an unreadable image."); return NULL; }
        uint8_t *bytes = (uint8_t *)malloc(jpeg.length);
        if (!bytes) { setError(errorOut, @"Out of memory."); return NULL; }
        memcpy(bytes, jpeg.bytes, jpeg.length);
        *length = jpeg.length;
        return bytes;
    }
}

static NSString *videoPath(PHAsset *asset, BOOL network, PHVideoRequestOptionsVersion version, NSString **error) {
    PHVideoRequestOptions *options = [[PHVideoRequestOptions alloc] init];
    options.networkAccessAllowed = network;
    options.version = version;
    options.deliveryMode = PHVideoRequestOptionsDeliveryModeHighQualityFormat;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    __block AVAsset *video;
    __block NSDictionary *details;
    PHImageRequestID request = [[PHImageManager defaultManager] requestAVAssetForVideo:asset options:options
        resultHandler:^(AVAsset *value, __unused AVAudioMix *mix, NSDictionary *info) {
        video = value;
        details = info;
        dispatch_semaphore_signal(done);
    }];
    if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 300 * NSEC_PER_SEC)) != 0) {
        [[PHImageManager defaultManager] cancelImageRequest:request];
        *error = @"Photos took too long to load this video.";
        return nil;
    }
    if ([video isKindOfClass:AVURLAsset.class]) return ((AVURLAsset *)video).URL.path;
    if (!video && [details[PHImageResultIsInCloudKey] boolValue] && !network) return nil;
    if (!video) *error = [details[PHImageErrorKey] localizedDescription] ?: @"Photos could not load this video.";
    return nil;
}

/// Writes the asset's paired Live Photo video into `cacheDir` once and returns its path.
static NSString *liveVideoPath(PHAsset *asset, BOOL network, NSString *cacheDir, NSString **error) {
    PHAssetResource *paired = nil;
    for (PHAssetResource *resource in [PHAssetResource assetResourcesForAsset:asset]) {
        if (resource.type == PHAssetResourceTypeFullSizePairedVideo) { paired = resource; break; }
        if (resource.type == PHAssetResourceTypePairedVideo && !paired) paired = resource;
    }
    if (!paired) { *error = @"This Live Photo has no motion video."; return nil; }
    NSString *stamp = [NSString stringWithFormat:@"%lld", (long long)asset.modificationDate.timeIntervalSince1970];
    NSString *name = [[asset.localIdentifier stringByReplacingOccurrencesOfString:@"/" withString:@"_"]
        stringByAppendingFormat:@"-%@-%ld.mov", stamp, (long)paired.type];
    NSString *path = [cacheDir stringByAppendingPathComponent:name];
    NSFileManager *files = NSFileManager.defaultManager;
    if ([files fileExistsAtPath:path]) return path;
    [files createDirectoryAtPath:cacheDir withIntermediateDirectories:YES attributes:nil error:NULL];
    NSString *partial = [path stringByAppendingFormat:@".%@.part", NSUUID.UUID.UUIDString];
    PHAssetResourceRequestOptions *options = [[PHAssetResourceRequestOptions alloc] init];
    options.networkAccessAllowed = network;
    dispatch_semaphore_t done = dispatch_semaphore_create(0);
    __block NSError *failure;
    [[PHAssetResourceManager defaultManager] writeDataForAssetResource:paired toFile:[NSURL fileURLWithPath:partial]
        options:options completionHandler:^(NSError *value) {
        failure = value;
        dispatch_semaphore_signal(done);
    }];
    if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 120 * NSEC_PER_SEC)) != 0 || failure) {
        [files removeItemAtPath:partial error:NULL];
        *error = failure.localizedDescription ?: @"Photos took too long to load this Live Photo.";
        return nil;
    }
    [files removeItemAtPath:path error:NULL];
    if (![files moveItemAtPath:partial toPath:path error:NULL]) {
        [files removeItemAtPath:partial error:NULL];
        *error = @"Could not cache the Live Photo motion.";
        return nil;
    }
    return path;
}

/// kind 0: the asset's video, read in place from the Photos library.
/// kind 1: a Live Photo's motion clip, cached under `cacheDir`.
/// Returns a malloc'd path or NULL; *errorOut stays NULL for a disallowed iCloud download.
extern "C" char *lap_photos_media_path(const char *identifier, int kind, const char *cacheDir, bool network, char **errorOut) {
    @autoreleasepool {
        NSString *error = nil;
        if (!authorize(&error)) { setError(errorOut, error); return NULL; }
        PHAsset *asset = sourceAsset(identifier);
        if (!asset) { setError(errorOut, @"This item is no longer in Apple Photos."); return NULL; }
        NSString *path = nil;
        if (kind == 1) {
            path = liveVideoPath(asset, network, [NSString stringWithUTF8String:cacheDir], &error);
        } else if (asset.mediaType == PHAssetMediaTypeVideo) {
            path = videoPath(asset, network, PHVideoRequestOptionsVersionCurrent, &error);
            // Slow-motion edits come back as compositions with no file; play the original.
            if (!path && !error) path = videoPath(asset, network, PHVideoRequestOptionsVersionOriginal, &error);
        } else {
            error = @"This item is not a video.";
        }
        if (path) return strdup(path.UTF8String);
        if (error) setError(errorOut, error);
        return NULL;
    }
}

static NSDictionary *assetDetails(PHAsset *asset) {
    NSArray<PHAssetResource *> *resources = [PHAssetResource assetResourcesForAsset:asset];
    NSString *filename = nil;
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
    for (PHAssetResource *resource in resources) {
        if (resource.type == PHAssetResourceTypePhoto || resource.type == PHAssetResourceTypeVideo) {
            filename = resource.originalFilename;
            break;
        }
    }
    if (!filename) filename = resources.firstObject.originalFilename;
#pragma clang diagnostic pop
    CLLocation *location = asset.location;
    return @{
        @"id": asset.localIdentifier,
        @"modified": @((long long)asset.modificationDate.timeIntervalSince1970),
        @"filename": filename ?: @"",
        @"created": asset.creationDate ? @((long long)asset.creationDate.timeIntervalSince1970) : NSNull.null,
        @"video": @(asset.mediaType == PHAssetMediaTypeVideo),
        @"live": @((asset.mediaSubtypes & PHAssetMediaSubtypePhotoLive) != 0),
        @"width": @(asset.pixelWidth), @"height": @(asset.pixelHeight),
        @"duration": @(asset.duration),
        @"favorite": @(asset.favorite),
        @"latitude": location ? @(location.coordinate.latitude) : NSNull.null,
        @"longitude": location ? @(location.coordinate.longitude) : NSNull.null,
        @"altitude": location ? @(location.altitude) : NSNull.null,
    };
}

/// Lists library assets as JSON: {"limited": bool, "assets": [...]}.
/// Request keys: "ids" (only these local identifiers), "everything" (include
/// hidden and shared assets), "details" (filenames, dates, size and location).
extern "C" char *lap_photos_list(const char *requestJson, char **errorOut) {
    @autoreleasepool {
        NSString *error = nil;
        if (!authorize(&error)) { setError(errorOut, error); return NULL; }
        NSData *data = [[NSString stringWithUTF8String:requestJson] dataUsingEncoding:NSUTF8StringEncoding];
        NSDictionary *request = [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
        if (![request isKindOfClass:NSDictionary.class]) { setError(errorOut, @"Invalid request."); return NULL; }
        BOOL everything = [request[@"everything"] boolValue];
        BOOL details = [request[@"details"] boolValue];
        PHFetchOptions *options = [[PHFetchOptions alloc] init];
        options.includeAllBurstAssets = everything;
        options.includeHiddenAssets = everything;
        options.includeAssetSourceTypes = everything
            ? (PHAssetSourceTypeUserLibrary | PHAssetSourceTypeCloudShared | PHAssetSourceTypeiTunesSynced)
            : PHAssetSourceTypeUserLibrary;
        NSArray *ids = request[@"ids"];
        PHFetchResult<PHAsset *> *assets = [ids isKindOfClass:NSArray.class]
            ? [PHAsset fetchAssetsWithLocalIdentifiers:ids options:options]
            : [PHAsset fetchAssetsWithOptions:options];
        NSMutableArray *items = [NSMutableArray arrayWithCapacity:assets.count];
        for (PHAsset *asset in assets) {
            @autoreleasepool {
                if (asset.mediaType != PHAssetMediaTypeImage && asset.mediaType != PHAssetMediaTypeVideo) continue;
                [items addObject:details ? assetDetails(asset) : @{
                    @"id": asset.localIdentifier,
                    @"modified": @((long long)asset.modificationDate.timeIntervalSince1970)}];
            }
        }
        BOOL limited = [PHPhotoLibrary authorizationStatusForAccessLevel:PHAccessLevelReadWrite] == PHAuthorizationStatusLimited;
        NSData *json = [NSJSONSerialization dataWithJSONObject:@{@"limited": @(limited), @"assets": items} options:0 error:NULL];
        return strdup([[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding].UTF8String);
    }
}

/// Change counter for the System Photo Library; starts observing on first call.
/// Returns -1 when Lap has no Photos access (never prompts).
extern "C" int64_t lap_photos_revision(void) {
    PHAuthorizationStatus status = [PHPhotoLibrary authorizationStatusForAccessLevel:PHAccessLevelReadWrite];
    if (status != PHAuthorizationStatusAuthorized && status != PHAuthorizationStatusLimited) return -1;
    static LapPhotosObserver *observer;
    static dispatch_once_t once;
    dispatch_once(&once, ^{
        observer = [[LapPhotosObserver alloc] init];
        [[PHPhotoLibrary sharedPhotoLibrary] registerChangeObserver:observer];
    });
    return (int64_t)libraryRevision.load();
}

// ---------------------------------------------------------------------------
// Writing back to Apple Photos. Each call performs one synchronous change
// block; macOS shows its own confirmation where Photos requires one.

static BOOL performChanges(dispatch_block_t changes, NSString **error) {
    NSError *failure = nil;
    BOOL ok = [[PHPhotoLibrary sharedPhotoLibrary] performChangesAndWait:changes error:&failure];
    if (!ok) *error = failure.localizedDescription ?: @"Apple Photos did not accept the change.";
    return ok;
}

/// Request JSON: {"ids": [...], "favorite": bool?, "location": {"latitude","longitude"} | null?,
/// "created": seconds?}. Keys that are absent are left unchanged; "location": null clears it.
extern "C" bool lap_photos_update(const char *requestJson, char **errorOut) {
    @autoreleasepool {
        NSString *error = nil;
        if (!authorize(&error)) { setError(errorOut, error); return false; }
        NSData *data = [[NSString stringWithUTF8String:requestJson] dataUsingEncoding:NSUTF8StringEncoding];
        NSDictionary *request = [NSJSONSerialization JSONObjectWithData:data options:0 error:NULL];
        NSArray *ids = request[@"ids"];
        if (![ids isKindOfClass:NSArray.class] || ids.count == 0) return true;
        PHFetchOptions *options = [[PHFetchOptions alloc] init];
        options.includeHiddenAssets = YES;
        options.includeAllBurstAssets = YES;
        PHFetchResult<PHAsset *> *assets = [PHAsset fetchAssetsWithLocalIdentifiers:ids options:options];
        if (assets.count == 0) return true;
        id favorite = request[@"favorite"];
        id location = request[@"location"];
        BOOL hasLocation = request[@"location"] != nil;
        id created = request[@"created"];
        BOOL ok = performChanges(^{
            for (PHAsset *asset in assets) {
                PHAssetChangeRequest *change = [PHAssetChangeRequest changeRequestForAsset:asset];
                if ([favorite isKindOfClass:NSNumber.class]) change.favorite = [favorite boolValue];
                if (hasLocation) {
                    change.location = [location isKindOfClass:NSDictionary.class]
                        ? [[CLLocation alloc] initWithLatitude:[location[@"latitude"] doubleValue]
                                                     longitude:[location[@"longitude"] doubleValue]]
                        : nil;
                }
                if ([created isKindOfClass:NSNumber.class]) {
                    change.creationDate = [NSDate dateWithTimeIntervalSince1970:[created doubleValue]];
                }
            }
        }, &error);
        if (!ok) setError(errorOut, error);
        return ok;
    }
}

/// Saves `jpegPath` as the asset's current edit (revertible in Photos).
extern "C" bool lap_photos_save_edit(const char *identifier, const char *jpegPath, char **errorOut) {
    @autoreleasepool {
        NSString *error = nil;
        if (!authorize(&error)) { setError(errorOut, error); return false; }
        PHAsset *asset = sourceAsset(identifier);
        if (!asset) { setError(errorOut, @"This item is no longer in Apple Photos."); return false; }
        if (![asset canPerformEditOperation:PHAssetEditOperationContent]) {
            setError(errorOut, @"Apple Photos does not allow editing this item.");
            return false;
        }
        PHContentEditingInputRequestOptions *options = [[PHContentEditingInputRequestOptions alloc] init];
        options.networkAccessAllowed = YES;
        options.canHandleAdjustmentData = ^BOOL(__unused PHAdjustmentData *adjustment) { return NO; };
        dispatch_semaphore_t done = dispatch_semaphore_create(0);
        __block PHContentEditingInput *input;
        // The completion handler runs on the main queue; this call runs on a worker thread.
        [asset requestContentEditingInputWithOptions:options completionHandler:^(PHContentEditingInput *value, __unused NSDictionary *info) {
            input = value;
            dispatch_semaphore_signal(done);
        }];
        if (dispatch_semaphore_wait(done, dispatch_time(DISPATCH_TIME_NOW, 300 * NSEC_PER_SEC)) != 0 || !input) {
            setError(errorOut, @"Apple Photos could not prepare this item for editing.");
            return false;
        }
        PHContentEditingOutput *output = [[PHContentEditingOutput alloc] initWithContentEditingInput:input];
        output.adjustmentData = [[PHAdjustmentData alloc] initWithFormatIdentifier:@"app.lap.edit" formatVersion:@"1"
            data:[@"Edited in Lap" dataUsingEncoding:NSUTF8StringEncoding]];
        NSData *jpeg = [NSData dataWithContentsOfFile:[NSString stringWithUTF8String:jpegPath]];
        NSError *writeError = nil;
        if (!jpeg || ![jpeg writeToURL:output.renderedContentURL options:NSDataWritingAtomic error:&writeError]) {
            setError(errorOut, writeError.localizedDescription ?: @"Could not write the edited image.");
            return false;
        }
        BOOL ok = performChanges(^{
            [PHAssetChangeRequest changeRequestForAsset:asset].contentEditingOutput = output;
        }, &error);
        if (!ok) setError(errorOut, error);
        return ok;
    }
}

/// Adds a photo or video file to the library; a paired video makes a Live Photo.
/// Returns the new local identifier (malloc'd) or NULL.
extern "C" char *lap_photos_create(const char *filePath, const char *pairedVideoPath, bool video, char **errorOut) {
    @autoreleasepool {
        NSString *error = nil;
        if (!authorize(&error)) { setError(errorOut, error); return NULL; }
        NSURL *file = [NSURL fileURLWithPath:[NSString stringWithUTF8String:filePath]];
        NSURL *paired = pairedVideoPath ? [NSURL fileURLWithPath:[NSString stringWithUTF8String:pairedVideoPath]] : nil;
        __block NSString *localId = nil;
        BOOL ok = performChanges(^{
            PHAssetCreationRequest *request = [PHAssetCreationRequest creationRequestForAsset];
            PHAssetResourceCreationOptions *options = [[PHAssetResourceCreationOptions alloc] init];
            options.originalFilename = file.lastPathComponent;
            [request addResourceWithType:video ? PHAssetResourceTypeVideo : PHAssetResourceTypePhoto fileURL:file options:options];
            if (paired) {
                PHAssetResourceCreationOptions *pairedOptions = [[PHAssetResourceCreationOptions alloc] init];
                pairedOptions.originalFilename = paired.lastPathComponent;
                [request addResourceWithType:PHAssetResourceTypePairedVideo fileURL:paired options:pairedOptions];
            }
            localId = request.placeholderForCreatedAsset.localIdentifier;
        }, &error);
        if (!ok || !localId) { setError(errorOut, error ?: @"Apple Photos did not add the file."); return NULL; }
        return strdup(localId.UTF8String);
    }
}

// ---------------------------------------------------------------------------
// iCloud Shared Albums.

static NSString *jsonString(id value) {
    NSData *json = [NSJSONSerialization dataWithJSONObject:value options:0 error:NULL];
    return [[NSString alloc] initWithData:json encoding:NSUTF8StringEncoding];
}

static PHFetchOptions *sharedAssetOptions() {
    PHFetchOptions *options = [[PHFetchOptions alloc] init];
    options.includeAssetSourceTypes = PHAssetSourceTypeCloudShared;
    options.sortDescriptors = @[[NSSortDescriptor sortDescriptorWithKey:@"creationDate" ascending:NO]];
    return options;
}

/// JSON [{"id","title","count","coverId"}] for the Shared Albums this Mac can see.
extern "C" char *lap_photos_shared_albums(char **errorOut) {
    @autoreleasepool {
        NSString *error = nil;
        if (!authorize(&error)) { setError(errorOut, error); return NULL; }
        NSMutableArray *albums = [NSMutableArray array];
        PHFetchResult<PHAssetCollection *> *collections = [PHAssetCollection
            fetchAssetCollectionsWithType:PHAssetCollectionTypeAlbum subtype:PHAssetCollectionSubtypeAlbumCloudShared options:nil];
        for (PHAssetCollection *album in collections) {
            PHFetchResult<PHAsset *> *assets = [PHAsset fetchAssetsInAssetCollection:album options:sharedAssetOptions()];
            [albums addObject:@{
                @"id": album.localIdentifier,
                @"title": album.localizedTitle ?: @"",
                @"count": @(assets.count),
                @"coverId": assets.firstObject.localIdentifier ?: NSNull.null,
            }];
        }
        [albums sortUsingDescriptors:@[[NSSortDescriptor sortDescriptorWithKey:@"title" ascending:YES selector:@selector(localizedCaseInsensitiveCompare:)]]];
        return strdup(jsonString(albums).UTF8String);
    }
}

/// JSON [{"id","filename","created","video","width","height"}] for one Shared Album, newest first.
extern "C" char *lap_photos_shared_album_assets(const char *albumId, char **errorOut) {
    @autoreleasepool {
        NSString *error = nil;
        if (!authorize(&error)) { setError(errorOut, error); return NULL; }
        PHAssetCollection *album = [PHAssetCollection
            fetchAssetCollectionsWithLocalIdentifiers:@[[NSString stringWithUTF8String:albumId]] options:nil].firstObject;
        if (!album) { setError(errorOut, @"This Shared Album is no longer available."); return NULL; }
        NSMutableArray *items = [NSMutableArray array];
        for (PHAsset *asset in [PHAsset fetchAssetsInAssetCollection:album options:sharedAssetOptions()]) {
            @autoreleasepool {
                NSDictionary *details = assetDetails(asset);
                [items addObject:@{
                    @"id": details[@"id"], @"filename": details[@"filename"], @"created": details[@"created"],
                    @"video": details[@"video"], @"width": details[@"width"], @"height": details[@"height"],
                }];
            }
        }
        return strdup(jsonString(items).UTF8String);
    }
}
