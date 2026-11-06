//! Verification code / link autofill, ported from
//! [linuxautofill](https://github.com/craftyderpysmall/linuxautofill).

pub mod detect;

pub use detect::{DEFAULT_KEYWORDS, Link, find_code, find_code_with, find_link, plain_links};
