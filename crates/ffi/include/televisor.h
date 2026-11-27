// C ABI of televisor-ffi. Hand-written: two functions, no generator needed.
// The wire format (JSON requests and responses) is documented in
// crates/ffi/src/protocol.rs.
#pragma once

// Run one request and return its JSON response. Blocks the calling thread
// until the request completes, so call it off the main thread. `request` is
// a NUL-terminated UTF-8 JSON object. Never returns NULL. The caller owns
// the result and must release it with `televisor_string_free`.
char *televisor_call(const char *request);

// Release a string returned by `televisor_call`. NULL is a no-op.
void televisor_string_free(char *string);
