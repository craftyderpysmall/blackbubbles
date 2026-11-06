//! Anisette from this Mac itself, via the private AOSKit framework
//! (`AOSUtilities retrieveOTPHeadersForDSID:`), the same call OpenHaystack relied on.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};
use bb_findmy::anisette::{Headers, Provider};
use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::AnyClass;
use objc2_foundation::{NSDictionary, NSString};

pub struct Native;

const AOSKIT: &std::ffi::CStr = c"/System/Library/PrivateFrameworks/AOSKit.framework/AOSKit";

unsafe extern "C" {
    fn dlopen(path: *const std::ffi::c_char, mode: i32) -> *mut std::ffi::c_void;
}

impl Native {
    pub fn new() -> Result<Self> {
        // RTLD_NOW = 2
        if unsafe { dlopen(AOSKIT.as_ptr(), 2) }.is_null() {
            bail!("could not load AOSKit");
        }
        AnyClass::get(c"AOSUtilities").context("AOSUtilities class missing from AOSKit")?;
        Ok(Self)
    }
}

impl Provider for Native {
    fn otp_headers(&self) -> Result<Headers> {
        let class = AnyClass::get(c"AOSUtilities").context("AOSUtilities class missing")?;
        let dsid = NSString::from_str("-2");
        let headers: Option<Retained<NSDictionary<NSString, NSString>>> =
            unsafe { msg_send![class, retrieveOTPHeadersForDSID: &*dsid] };
        let headers = headers.context("AOSKit returned no anisette headers")?;
        let get = |key: &str| -> Result<String> {
            headers
                .objectForKey(&NSString::from_str(key))
                .map(|v| v.to_string())
                .with_context(|| format!("AOSKit anisette lacks {key}"))
        };
        let mut out = BTreeMap::new();
        out.insert("X-Apple-I-MD".to_string(), get("X-Apple-MD")?);
        out.insert("X-Apple-I-MD-M".to_string(), get("X-Apple-MD-M")?);
        Ok(out)
    }
}
