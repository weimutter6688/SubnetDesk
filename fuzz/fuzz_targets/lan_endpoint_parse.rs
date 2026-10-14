#![no_main]
//! User-typed and discovered endpoints go through `Endpoint::parse`. Must never panic,
//! and a successfully parsed endpoint must re-parse from its own authority.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(input) = std::str::from_utf8(data) else {
        return;
    };
    if let Ok(endpoint) = hbb_common::lan::Endpoint::parse(input) {
        let reparsed = hbb_common::lan::Endpoint::parse(endpoint.authority())
            .expect("an endpoint's own authority must parse");
        assert_eq!(reparsed.port(), endpoint.port());
    }
});
