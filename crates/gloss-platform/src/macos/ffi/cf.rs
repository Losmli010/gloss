//! 核心基础框架（CoreFoundation）：本层用到的符号、核心基础对象守卫与字符串及数据转换助手。

use std::ffi::CStr;

pub(crate) use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
pub(crate) use core_foundation_sys::base::{
    Boolean, CFGetTypeID, CFIndex, CFRange, CFRelease, CFRetain, CFTypeRef, kCFAllocatorDefault,
};
pub(crate) use core_foundation_sys::data::{
    CFDataCreate, CFDataGetBytePtr, CFDataGetLength, CFDataRef,
};
pub(crate) use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
pub(crate) use core_foundation_sys::dictionary::{
    CFDictionaryCreate, CFDictionaryRef, kCFTypeDictionaryKeyCallBacks,
    kCFTypeDictionaryValueCallBacks,
};
pub(crate) use core_foundation_sys::mach_port::{CFMachPortCreateRunLoopSource, CFMachPortRef};
pub(crate) use core_foundation_sys::number::kCFBooleanTrue;
pub(crate) use core_foundation_sys::runloop::{
    CFRunLoopAddSource, CFRunLoopAddTimer, CFRunLoopGetCurrent, CFRunLoopRef, CFRunLoopRun,
    CFRunLoopStop, CFRunLoopTimerContext, CFRunLoopTimerCreate, CFRunLoopTimerInvalidate,
    CFRunLoopTimerRef, kCFRunLoopCommonModes,
};
pub(crate) use core_foundation_sys::string::{
    CFStringCreateWithCString, CFStringGetBytes, CFStringGetLength,
    CFStringGetMaximumSizeForEncoding, CFStringGetTypeID, CFStringRef, kCFStringEncodingUTF8,
};

pub(crate) struct CfGuard(CFTypeRef);

impl CfGuard {
    pub(crate) fn new(reference: CFTypeRef) -> Self {
        Self(reference)
    }

    pub(crate) fn raw(&self) -> CFTypeRef {
        self.0
    }

    pub(crate) fn string_ref(&self) -> CFStringRef {
        self.0 as CFStringRef
    }

    pub(crate) fn data_ref(&self) -> CFDataRef {
        self.0 as CFDataRef
    }
}

impl Drop for CfGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: construction only takes over the +1 reference (or NULL)
            // returned by create/copy/Retain, the guard is the sole release
            // point, and it gives the reference back exactly once.
            unsafe { CFRelease(self.0) };
        }
    }
}

pub(crate) fn cf_string(name: &CStr) -> Option<CfGuard> {
    // SAFETY: `name` is a valid NUL-terminated C string with the supported
    // UTF-8 encoding; allocation failure returns NULL, which is handled as
    // failure here.
    let s = unsafe {
        CFStringCreateWithCString(kCFAllocatorDefault, name.as_ptr(), kCFStringEncodingUTF8)
    };
    (!s.is_null()).then(|| CfGuard::new(s as CFTypeRef))
}

#[cfg(test)]
pub(crate) fn cf_data(bytes: &[u8]) -> Option<CfGuard> {
    // SAFETY: `bytes` stays alive for the duration of this call and is not
    // dangling, with the length matching the buffer; allocation failure returns
    // NULL, which is handled as failure here.
    let data = unsafe { CFDataCreate(kCFAllocatorDefault, bytes.as_ptr(), bytes.len() as CFIndex) };
    (!data.is_null()).then(|| CfGuard::new(data as CFTypeRef))
}

pub(crate) unsafe fn string_from(value: CFTypeRef, max_bytes: usize) -> Option<String> {
    let s = value as CFStringRef;
    if s.is_null() {
        return None;
    }
    // SAFETY: `s` is non-null and a valid CF object; `GetTypeID` is safe on any
    // CF object, and the CFString-specific API is only called after the type is
    // confirmed.
    if unsafe { CFGetTypeID(s as CFTypeRef) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    // SAFETY: `s` has been confirmed to be a CFString; the length query has no
    // preconditions.
    let len = unsafe { CFStringGetLength(s) };
    if len == 0 {
        return Some(String::new());
    }
    // SAFETY: UTF-8 is a supported encoding, and the call returns a non-negative
    // upper bound of the encoded byte count.
    let max = unsafe { CFStringGetMaximumSizeForEncoding(len, kCFStringEncodingUTF8) };
    let Ok(max) = usize::try_from(max) else {
        return None;
    };
    if max > max_bytes {
        return None;
    }
    let mut buf = vec![0u8; max];
    let mut used: CFIndex = 0;
    // SAFETY: `s` is a valid CFString; the buffer capacity is exactly the bound
    // `max`, so `GetBytes` writes in bounds; lossByte=0 means the return value is
    // smaller than the requested length when the string cannot be fully converted.
    let converted = unsafe {
        CFStringGetBytes(
            s,
            CFRange {
                location: 0,
                length: len,
            },
            kCFStringEncodingUTF8,
            0,
            0,
            buf.as_mut_ptr(),
            max as CFIndex,
            &mut used,
        )
    };
    if converted != len {
        return None;
    }
    let used = usize::try_from(used).ok()?;
    if used > buf.len() {
        return None;
    }
    buf.truncate(used);
    String::from_utf8(buf).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cf_string_round_trips_back_to_utf8() {
        let guard = cf_string(c"gloss 往返回环").expect("cf string");

        // SAFETY: the guard holds a valid +1 reference; the bound is generous
        // enough to hold the whole string.
        let text = unsafe { string_from(guard.string_ref() as CFTypeRef, 1024) };
        assert_eq!(text.as_deref(), Some("gloss 往返回环"));
    }

    #[test]
    fn string_from_rejects_non_string_objects() {
        let guard = cf_data(b"not a string").expect("cf data");

        // SAFETY: the guard holds a valid +1 reference; the bound is deliberately
        // maximal so the type check is the only possible reason for rejection.
        let text = unsafe { string_from(guard.data_ref() as CFTypeRef, usize::MAX) };
        assert_eq!(text, None);
    }

    #[test]
    fn string_from_respects_the_byte_ceiling() {
        let guard = cf_string(c"gloss").expect("cf string");

        // SAFETY: the guard holds a valid +1 reference; the bound is deliberately
        // smaller than actually needed.
        let text = unsafe { string_from(guard.string_ref() as CFTypeRef, 2) };
        assert_eq!(text, None);
    }

    #[test]
    fn empty_cf_string_converts_to_empty_rust_string() {
        let guard = cf_string(c"").expect("cf string");

        // SAFETY: the guard holds a valid +1 reference; an empty string needs no
        // buffer.
        let text = unsafe { string_from(guard.string_ref() as CFTypeRef, 8) };
        assert_eq!(text.as_deref(), Some(""));
    }
}
