#import <Foundation/Foundation.h>
#import <Vision/Vision.h>

extern "C" {

/// Detect cats and dogs with Apple Vision in an encoded image (JPEG/PNG/...).
/// Writes up to `max_results` detections to `out`, 6 floats each:
/// x, y, width, height (normalized, top-left origin), kind (0 = cat, 1 = dog), confidence.
/// Returns the number of detections, or -1 on error (message in `error_out`, free with lap_vision_free_string).
int lap_vision_detect_animals(const uint8_t *data, size_t len, float *out, int max_results, char **error_out) {
    @autoreleasepool {
        if (!data || len == 0 || !out || max_results <= 0) return 0;
        NSData *imageData = [NSData dataWithBytesNoCopy:(void *)data length:len freeWhenDone:NO];
        VNImageRequestHandler *handler = [[VNImageRequestHandler alloc] initWithData:imageData options:@{}];
        VNRecognizeAnimalsRequest *request = [[VNRecognizeAnimalsRequest alloc] init];
        NSError *error = nil;
        if (![handler performRequests:@[request] error:&error]) {
            if (error_out) {
                NSString *message = error.localizedDescription ?: @"Vision request failed";
                *error_out = strdup(message.UTF8String);
            }
            return -1;
        }
        int count = 0;
        for (VNRecognizedObjectObservation *observation in request.results) {
            if (count >= max_results) break;
            VNClassificationObservation *best = observation.labels.firstObject;
            if (!best) continue;
            float kind;
            if ([best.identifier isEqualToString:VNAnimalIdentifierCat]) {
                kind = 0.0f;
            } else if ([best.identifier isEqualToString:VNAnimalIdentifierDog]) {
                kind = 1.0f;
            } else {
                continue;
            }
            CGRect box = observation.boundingBox; // normalized, bottom-left origin
            float *row = out + count * 6;
            row[0] = (float)box.origin.x;
            row[1] = (float)(1.0 - box.origin.y - box.size.height);
            row[2] = (float)box.size.width;
            row[3] = (float)box.size.height;
            row[4] = kind;
            row[5] = best.confidence;
            count++;
        }
        return count;
    }
}

void lap_vision_free_string(char *ptr) {
    free(ptr);
}

}
