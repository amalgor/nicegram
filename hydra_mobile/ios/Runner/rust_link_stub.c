// Keeps cargokit static Rust linked in Release/TestFlight when Xcode uses -dead_strip.
// Without a root reference, the linker may strip code that Dart resolves dynamically.

#include <stdint.h>

extern int64_t frb_get_rust_content_hash(void);

__attribute__((used, retain))
static const void *const kHydraRustLinkAnchor =
    (const void *)(uintptr_t)frb_get_rust_content_hash;
