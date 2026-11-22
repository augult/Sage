THOUGHT = "Drop the build directory and finish."

CONTENT = "Implemented src/lib.rs: Xorcism<'a> holds a borrowed key slice and a cursor, `new` accepts any ?Sized AsRef<[u8]>, and a private next_byte helper drives both `munge_in_place` and the lazy, allocation-free `munge` iterator (generic over IntoIterator with Borrow<u8> items, using a precise-capture return so lifetimes tie to &mut self). Under the `io` feature, `reader`/`writer` wrap Read/Write and reuse the same cursor. Builds clean with and without `--features io`; temporary tests covering both munge paths, involution, borrowed/owned inputs, statefulness and the io adaptors all passed before removal."

CALLS = []
