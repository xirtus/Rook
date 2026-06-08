// AVFoundation Shim for Rook
// Provides C-callable wrappers around AVFoundation, VideoToolbox, and IOSurface.

#import <AVFoundation/AVFoundation.h>
#import <CoreMedia/CoreMedia.h>
#import <CoreVideo/CoreVideo.h>
#import <VideoToolbox/VideoToolbox.h>
#import <IOSurface/IOSurface.h>
#import <Foundation/Foundation.h>
#import <stddef.h>

// ─── AVFoundationContext (matches Rust repr(C) struct) ───────
typedef struct {
    void* asset;
    void* reader;
    void* track_output;
    int32_t time_scale;
    double nominal_fps;
    double timecode_base;
} AVFoundationContext;

// ─── VideoPropertiesC (matches Rust repr(C) struct) ───────
typedef struct {
    int32_t width;
    int32_t height;
    double duration;
    double frame_rate;
    int32_t time_scale;
} VideoPropertiesC;

// ─── Uncaught exception handler ───────────────────────────
static void avf_uncaught_exception_handler(NSException* exception) {
    NSLog(@"[Rook] Uncaught ObjC exception: %@ reason: %@", exception.name, exception.reason);
}

void avf_install_uncaught_exception_handler(void) {
    NSSetUncaughtExceptionHandler(&avf_uncaught_exception_handler);
}

// ─── AVFoundation context management ──────────────────────

void* avfoundation_create_context(const char* video_path) {
    @autoreleasepool {
        @try {
            NSString* path = [NSString stringWithUTF8String:video_path];
            if (!path) return NULL;

            NSURL* url = [NSURL fileURLWithPath:path];
            AVAsset* asset = [AVAsset assetWithURL:url];

            NSError* error = nil;
            AVKeyValueStatus status = [asset statusOfValueForKey:@"tracks" error:&error];
            if (status == AVKeyValueStatusFailed) {
                NSLog(@"[Rook] Failed to load asset tracks: %@", error);
                return NULL;
            }

            if (status == AVKeyValueStatusLoading || status == AVKeyValueStatusUnknown) {
                dispatch_semaphore_t sema = dispatch_semaphore_create(0);
                [asset loadValuesAsynchronouslyForKeys:@[@"tracks", @"duration"]
                                     completionHandler:^{ dispatch_semaphore_signal(sema); }];
                dispatch_semaphore_wait(sema, dispatch_time(DISPATCH_TIME_NOW, 5 * NSEC_PER_SEC));
            }

            NSArray<AVAssetTrack*>* videoTracks = [asset tracksWithMediaType:AVMediaTypeVideo];
            if (videoTracks.count == 0) {
                NSLog(@"[Rook] No video track found");
                return NULL;
            }
            AVAssetTrack* videoTrack = videoTracks[0];

            AVAssetReader* reader = [[AVAssetReader alloc] initWithAsset:asset error:&error];
            if (!reader) {
                NSLog(@"[Rook] Failed to create AVAssetReader: %@", error);
                return NULL;
            }

            NSDictionary* outputSettings = @{
                (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange),
                (id)kCVPixelBufferIOSurfacePropertiesKey: @{},
            };

            AVAssetReaderTrackOutput* trackOutput =
                [[AVAssetReaderTrackOutput alloc] initWithTrack:videoTrack
                                                  outputSettings:outputSettings];
            trackOutput.alwaysCopiesSampleData = NO;

            if (![reader canAddOutput:trackOutput]) {
                NSLog(@"[Rook] Cannot add track output to reader");
                return NULL;
            }
            [reader addOutput:trackOutput];

            double fps = videoTrack.nominalFrameRate;
            if (fps <= 0) fps = 30.0;

            AVFoundationContext* ctx = (AVFoundationContext*)calloc(1, sizeof(AVFoundationContext));
            ctx->asset = (__bridge_retained void*)asset;
            ctx->reader = (__bridge_retained void*)reader;
            ctx->track_output = (__bridge_retained void*)trackOutput;
            ctx->time_scale = (int32_t)videoTrack.naturalTimeScale;
            ctx->nominal_fps = fps;
            ctx->timecode_base = 0.0;

            return ctx;
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_create_context exception: %@ %@", e.name, e.reason);
            return NULL;
        }
    }
}

int avfoundation_get_video_properties(void* ctx_ptr, VideoPropertiesC* props) {
    if (!ctx_ptr || !props) return -1;
    @autoreleasepool {
        @try {
            AVFoundationContext* ctx = (AVFoundationContext*)ctx_ptr;
            AVAsset* asset = (__bridge AVAsset*)ctx->asset;
            AVAssetTrack* videoTrack = [[asset tracksWithMediaType:AVMediaTypeVideo] firstObject];
            if (!videoTrack) return -1;

            CGSize naturalSize = videoTrack.naturalSize;
            CGAffineTransform t = videoTrack.preferredTransform;
            if (t.b == 1.0 || t.b == -1.0) {
                props->width = (int32_t)naturalSize.height;
                props->height = (int32_t)naturalSize.width;
            } else {
                props->width = (int32_t)naturalSize.width;
                props->height = (int32_t)naturalSize.height;
            }

            CMTime duration = asset.duration;
            props->duration = CMTimeGetSeconds(duration);
            props->frame_rate = ctx->nominal_fps;
            props->time_scale = ctx->time_scale;
            return 0;
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_get_video_properties exception: %@ %@", e.name, e.reason);
            return -1;
        }
    }
}

void* avfoundation_copy_track_format_desc(void* ctx_ptr) {
    if (!ctx_ptr) return NULL;
    @autoreleasepool {
        @try {
            AVFoundationContext* ctx = (AVFoundationContext*)ctx_ptr;
            AVAsset* asset = (__bridge AVAsset*)ctx->asset;
            AVAssetTrack* videoTrack = [[asset tracksWithMediaType:AVMediaTypeVideo] firstObject];
            if (!videoTrack) return NULL;
            NSArray* formatDescriptions = videoTrack.formatDescriptions;
            if (formatDescriptions.count == 0) return NULL;
            CMFormatDescriptionRef desc = (__bridge CMFormatDescriptionRef)formatDescriptions[0];
            CFRetain(desc);
            return (void*)desc;
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_copy_track_format_desc exception: %@ %@", e.name, e.reason);
            return NULL;
        }
    }
}

void* avfoundation_read_next_sample(void* ctx_ptr) {
    if (!ctx_ptr) return NULL;
    @autoreleasepool {
        @try {
            AVFoundationContext* ctx = (AVFoundationContext*)ctx_ptr;
            AVAssetReaderTrackOutput* trackOutput = (__bridge AVAssetReaderTrackOutput*)ctx->track_output;
            CMSampleBufferRef sample = [trackOutput copyNextSampleBuffer];
            return (void*)sample;
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_read_next_sample exception: %@ %@", e.name, e.reason);
            return NULL;
        }
    }
}

int avfoundation_get_reader_status(void* ctx_ptr) {
    if (!ctx_ptr) return 3;
    @autoreleasepool {
        @try {
            AVFoundationContext* ctx = (AVFoundationContext*)ctx_ptr;
            AVAssetReader* reader = (__bridge AVAssetReader*)ctx->reader;
            return (int)reader.status;
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_get_reader_status exception: %@ %@", e.name, e.reason);
            return 3;
        }
    }
}

int avfoundation_seek_to(void* ctx_ptr, double timestamp_sec) {
    if (!ctx_ptr) return -1;
    @autoreleasepool {
        @try {
            AVFoundationContext* ctx = (AVFoundationContext*)ctx_ptr;
            AVAsset* asset = (__bridge AVAsset*)ctx->asset;

            // Release old reader/output
            if (ctx->reader) {
                AVAssetReader* oldReader = (__bridge_transfer AVAssetReader*)ctx->reader;
                if (oldReader.status == AVAssetReaderStatusReading) [oldReader cancelReading];
                ctx->reader = NULL;
            }
            if (ctx->track_output) {
                CFRelease(ctx->track_output);
                ctx->track_output = NULL;
            }

            NSError* error = nil;
            AVAssetReader* newReader = [[AVAssetReader alloc] initWithAsset:asset error:&error];
            if (!newReader) return -1;

            CMTime startTime = CMTimeMakeWithSeconds(timestamp_sec, ctx->time_scale > 0 ? ctx->time_scale : 600);
            newReader.timeRange = CMTimeRangeMake(startTime, kCMTimePositiveInfinity);

            AVAssetTrack* videoTrack = [[asset tracksWithMediaType:AVMediaTypeVideo] firstObject];
            if (!videoTrack) return -1;

            NSDictionary* outputSettings = @{
                (id)kCVPixelBufferPixelFormatTypeKey: @(kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange),
                (id)kCVPixelBufferIOSurfacePropertiesKey: @{},
            };

            AVAssetReaderTrackOutput* newOutput =
                [[AVAssetReaderTrackOutput alloc] initWithTrack:videoTrack
                                                  outputSettings:outputSettings];
            newOutput.alwaysCopiesSampleData = NO;

            if (![newReader canAddOutput:newOutput]) return -1;
            [newReader addOutput:newOutput];

            ctx->reader = (__bridge_retained void*)newReader;
            ctx->track_output = (__bridge_retained void*)newOutput;

            // Caller (Rust) will call avfoundation_start_reader explicitly.
            return 0;
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_seek_to exception: %@ %@", e.name, e.reason);
            return -1;
        }
    }
}

int avfoundation_start_reader(void* ctx_ptr) {
    if (!ctx_ptr) return -1;
    @autoreleasepool {
        @try {
            AVFoundationContext* ctx = (AVFoundationContext*)ctx_ptr;
            AVAssetReader* reader = (__bridge AVAssetReader*)ctx->reader;
            return [reader startReading] ? 0 : -1;
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_start_reader exception: %@ %@", e.name, e.reason);
            return -1;
        }
    }
}

double avfoundation_peek_first_sample_pts(void* ctx_ptr) {
    if (!ctx_ptr) return 0.0;
    @autoreleasepool {
        @try {
            AVFoundationContext* ctx = (AVFoundationContext*)ctx_ptr;
            AVAssetReaderTrackOutput* trackOutput = (__bridge AVAssetReaderTrackOutput*)ctx->track_output;
            CMSampleBufferRef sample = [trackOutput copyNextSampleBuffer];
            if (!sample) return 0.0;
            CMTime pts = CMSampleBufferGetPresentationTimeStamp(sample);
            CFRelease(sample);
            return CMTimeGetSeconds(pts);
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_peek_first_sample_pts exception: %@ %@", e.name, e.reason);
            return 0.0;
        }
    }
}

void avfoundation_release_context(void* ctx_ptr) {
    if (!ctx_ptr) return;
    @autoreleasepool {
        @try {
            AVFoundationContext* ctx = (AVFoundationContext*)ctx_ptr;
            if (ctx->track_output) { CFRelease(ctx->track_output); ctx->track_output = NULL; }
            if (ctx->reader) {
                AVAssetReader* reader = (__bridge AVAssetReader*)ctx->reader;
                if (reader.status == AVAssetReaderStatusReading) [reader cancelReading];
                CFRelease(ctx->reader); ctx->reader = NULL;
            }
            if (ctx->asset) { CFRelease(ctx->asset); ctx->asset = NULL; }
            free(ctx);
        } @catch (NSException* e) {
            NSLog(@"[Rook] avfoundation_release_context exception: %@ %@", e.name, e.reason);
            // Still free the struct even on exception to avoid leak
            free(ctx_ptr);
        }
    }
}

// ─── Destination attributes for VTDecompressionSession ─────

void* avfoundation_create_destination_attributes(void) {
    CFMutableDictionaryRef attrs = CFDictionaryCreateMutable(
        kCFAllocatorDefault, 2,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);

    int pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
    CFNumberRef pfNum = CFNumberCreate(kCFAllocatorDefault, kCFNumberIntType, &pixelFormat);
    CFDictionarySetValue(attrs, kCVPixelBufferPixelFormatTypeKey, pfNum);
    CFRelease(pfNum);

    CFDictionaryRef ioSurfaceProps = CFDictionaryCreate(
        kCFAllocatorDefault, NULL, NULL, 0,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);
    CFDictionarySetValue(attrs, kCVPixelBufferIOSurfacePropertiesKey, ioSurfaceProps);
    CFRelease(ioSurfaceProps);

    return attrs;
}

void* avfoundation_create_destination_attributes_scaled(int width, int height) {
    CFMutableDictionaryRef attrs = (CFMutableDictionaryRef)avfoundation_create_destination_attributes();

    if (width > 0) {
        CFNumberRef wNum = CFNumberCreate(kCFAllocatorDefault, kCFNumberIntType, &width);
        CFDictionarySetValue(attrs, kCVPixelBufferWidthKey, wNum);
        CFRelease(wNum);
    }
    if (height > 0) {
        CFNumberRef hNum = CFNumberCreate(kCFAllocatorDefault, kCFNumberIntType, &height);
        CFDictionarySetValue(attrs, kCVPixelBufferHeightKey, hNum);
        CFRelease(hNum);
    }

    return attrs;
}

// ─── VTDecompressionSession wrappers ──────────────────────

typedef void (*VTDecompressionCallback)(
    void* decompressionOutputRefCon,
    void* sourceFrameRefCon,
    int32_t status,
    uint32_t infoFlags,
    void* imageBuffer,
    CMTime presentationTimeStamp,
    CMTime presentationDuration);

int avf_vt_create_session(
    void* fmt,
    const void* dest_attrs,
    VTDecompressionCallback cb,
    void* refcon,
    void** out_sess)
{
    if (!fmt || !out_sess) return -1;

    VTDecompressionOutputCallbackRecord callbackRecord = {
        .decompressionOutputCallback = (VTDecompressionOutputCallback)cb,
        .decompressionOutputRefCon = refcon,
    };

    return VTDecompressionSessionCreate(
        kCFAllocatorDefault,
        (CMVideoFormatDescriptionRef)fmt,
        NULL,
        (CFDictionaryRef)dest_attrs,
        &callbackRecord,
        (VTDecompressionSessionRef*)out_sess);
}

int avf_vt_create_session_iosurface(
    void* fmt,
    VTDecompressionCallback cb,
    void* refcon,
    void** out_sess)
{
    if (!fmt || !out_sess) return -1;

    CMVideoFormatDescriptionRef formatDesc = (CMVideoFormatDescriptionRef)fmt;
    CMVideoDimensions dims = CMVideoFormatDescriptionGetDimensions(formatDesc);

    CFMutableDictionaryRef destAttrs = CFDictionaryCreateMutable(
        kCFAllocatorDefault, 4,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);

    int pixelFormat = kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange;
    CFNumberRef pfNum = CFNumberCreate(kCFAllocatorDefault, kCFNumberIntType, &pixelFormat);
    CFDictionarySetValue(destAttrs, kCVPixelBufferPixelFormatTypeKey, pfNum);
    CFRelease(pfNum);

    CFNumberRef wNum = CFNumberCreate(kCFAllocatorDefault, kCFNumberIntType, &dims.width);
    CFDictionarySetValue(destAttrs, kCVPixelBufferWidthKey, wNum);
    CFRelease(wNum);

    CFNumberRef hNum = CFNumberCreate(kCFAllocatorDefault, kCFNumberIntType, &dims.height);
    CFDictionarySetValue(destAttrs, kCVPixelBufferHeightKey, hNum);
    CFRelease(hNum);

    CFDictionaryRef ioSurfaceProps = CFDictionaryCreate(
        kCFAllocatorDefault, NULL, NULL, 0,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);
    CFDictionarySetValue(destAttrs, kCVPixelBufferIOSurfacePropertiesKey, ioSurfaceProps);
    CFRelease(ioSurfaceProps);

    VTDecompressionOutputCallbackRecord callbackRecord = {
        .decompressionOutputCallback = (VTDecompressionOutputCallback)cb,
        .decompressionOutputRefCon = refcon,
    };

    OSStatus status = VTDecompressionSessionCreate(
        kCFAllocatorDefault, formatDesc, NULL, destAttrs,
        &callbackRecord, (VTDecompressionSessionRef*)out_sess);

    CFRelease(destAttrs);
    return status;
}

int avf_vt_decode_frame(void* sess, void* sample) {
    if (!sess || !sample) return -1;
    return VTDecompressionSessionDecodeFrame(
        (VTDecompressionSessionRef)sess,
        (CMSampleBufferRef)sample,
        0, NULL, NULL);
}

void avf_vt_wait_async(void* sess) {
    if (sess) VTDecompressionSessionWaitForAsynchronousFrames((VTDecompressionSessionRef)sess);
}

void avf_vt_invalidate(void* sess) {
    if (sess) {
        VTDecompressionSessionInvalidate((VTDecompressionSessionRef)sess);
        CFRelease(sess);
    }
}

// ─── CVPixelBuffer / IOSurface helpers ────────────────────

void* avf_cvpixelbuffer_get_iosurface(void* pixel_buffer) {
    if (!pixel_buffer) return NULL;
    return CVPixelBufferGetIOSurface((CVPixelBufferRef)pixel_buffer);
}

void* avf_create_iosurface_destination_attributes(int width, int height) {
    return avfoundation_create_destination_attributes_scaled(width, height);
}

// ─── IOSurface accessors ──────────────────────────────────

void avf_iosurface_lock_readonly(void* surface) {
    if (surface) IOSurfaceLock((IOSurfaceRef)surface, kIOSurfaceLockReadOnly, NULL);
}

void avf_iosurface_unlock(void* surface) {
    if (surface) IOSurfaceUnlock((IOSurfaceRef)surface, kIOSurfaceLockReadOnly, NULL);
}

size_t avf_iosurface_width_of_plane(void* surface, size_t plane) {
    if (surface) return IOSurfaceGetWidthOfPlane((IOSurfaceRef)surface, plane);
    return 0;
}

size_t avf_iosurface_height_of_plane(void* surface, size_t plane) {
    if (surface) return IOSurfaceGetHeightOfPlane((IOSurfaceRef)surface, plane);
    return 0;
}

size_t avf_iosurface_bytes_per_row_of_plane(void* surface, size_t plane) {
    if (surface) return IOSurfaceGetBytesPerRowOfPlane((IOSurfaceRef)surface, plane);
    return 0;
}

void* avf_iosurface_base_address_of_plane(void* surface, size_t plane) {
    if (surface) return IOSurfaceGetBaseAddressOfPlane((IOSurfaceRef)surface, plane);
    return NULL;
}
