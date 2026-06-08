// AVFoundation Shim — header for Rook
#pragma once
#include <stddef.h>
void avf_iosurface_lock_readonly(void* surface);
void avf_iosurface_unlock(void* surface);
size_t avf_iosurface_width_of_plane(void* surface, size_t plane);
size_t avf_iosurface_height_of_plane(void* surface, size_t plane);
size_t avf_iosurface_bytes_per_row_of_plane(void* surface, size_t plane);
void* avf_iosurface_base_address_of_plane(void* surface, size_t plane);
