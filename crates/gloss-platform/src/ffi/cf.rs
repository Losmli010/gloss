//! CoreFoundation：本层用到的符号、CF 对象守卫与字符串/数据转换助手。

use std::ffi::CStr;

pub(crate) use core_foundation_sys::array::{CFArrayGetCount, CFArrayGetValueAtIndex, CFArrayRef};
pub(crate) use core_foundation_sys::base::{
    Boolean, CFGetTypeID, CFIndex, CFRange, CFRelease, CFRetain, CFTypeRef, kCFAllocatorDefault,
};
pub(crate) use core_foundation_sys::data::{
    CFDataCreate, CFDataGetBytePtr, CFDataGetLength, CFDataRef,
};
pub(crate) use core_foundation_sys::date::CFAbsoluteTimeGetCurrent;
pub(crate) use core_foundation_sys::mach_port::{CFMachPortCreateRunLoopSource, CFMachPortRef};
pub(crate) use core_foundation_sys::runloop::{
    CFRunLoopAddSource, CFRunLoopAddTimer, CFRunLoopGetCurrent, CFRunLoopRef, CFRunLoopRun,
    CFRunLoopStop, CFRunLoopTimerContext, CFRunLoopTimerCreate, CFRunLoopTimerInvalidate,
    CFRunLoopTimerRef, kCFRunLoopCommonModes,
};
pub(crate) use core_foundation_sys::string::{
    CFStringCreateWithCString, CFStringGetBytes, CFStringGetLength,
    CFStringGetMaximumSizeForEncoding, CFStringGetTypeID, CFStringRef, kCFStringEncodingUTF8,
};

/// CF 对象（+1 引用）的所有权守卫：出作用域即 CFRelease。
pub(crate) struct CfGuard(CFTypeRef);

impl CfGuard {
    /// 接管一个 +1 引用；`NULL` 合法（Drop 时判空跳过）。
    pub(crate) fn new(reference: CFTypeRef) -> Self {
        Self(reference)
    }

    /// 借用为 CFString 引用（调用方保证该对象确实是 CFString）。
    pub(crate) fn string_ref(&self) -> CFStringRef {
        self.0 as CFStringRef
    }

    /// 借用为 CFData 引用（调用方保证该对象确实是 CFData）。
    pub(crate) fn data_ref(&self) -> CFDataRef {
        self.0 as CFDataRef
    }
}

impl Drop for CfGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: 构造处只接管创建/拷贝/Retain 返回的 +1 引用（或 NULL），
            // 守卫是唯一释放点，恰好归还一次。
            unsafe { CFRelease(self.0) };
        }
    }
}

/// 按 C 字符串构造 CFString（+1 引用，由守卫释放）；分配失败返回 `None`。
pub(crate) fn cf_string(name: &CStr) -> Option<CfGuard> {
    // SAFETY: `name` 是 NUL 结尾的有效 C 字符串，编码为受支持的 UTF-8；
    // 分配失败返回 NULL，此处按失败处理。
    let s = unsafe {
        CFStringCreateWithCString(kCFAllocatorDefault, name.as_ptr(), kCFStringEncodingUTF8)
    };
    (!s.is_null()).then(|| CfGuard::new(s as CFTypeRef))
}

/// 按字节构造 CFData（+1 引用，由守卫释放）；分配失败返回 `None`。
#[cfg(test)]
pub(crate) fn cf_data(bytes: &[u8]) -> Option<CfGuard> {
    // SAFETY: `bytes` 在本次调用内存活且非悬空，长度即缓冲长度；
    // 分配失败返回 NULL，此处按失败处理。
    let data = unsafe { CFDataCreate(kCFAllocatorDefault, bytes.as_ptr(), bytes.len() as CFIndex) };
    (!data.is_null()).then(|| CfGuard::new(data as CFTypeRef))
}

/// 把 CF 对象转成 Rust 字符串：非 CFString、编码失败或 UTF-8 字节数超过
/// `max_bytes` 都返回 `None`（上界在分配缓冲之前判定）。
///
/// # Safety
///
/// `value` 是有效的 CF 对象引用或 NULL（内部先验类型）；+1 引用仍由调用方持有。
pub(crate) unsafe fn string_from(value: CFTypeRef, max_bytes: usize) -> Option<String> {
    let s = value as CFStringRef;
    if s.is_null() {
        return None;
    }
    // SAFETY: `s` 非空且为有效 CF 对象；GetTypeID 对任意 CF 对象安全，
    // 确认类型后才调用 CFString 专属 API。
    if unsafe { CFGetTypeID(s as CFTypeRef) } != unsafe { CFStringGetTypeID() } {
        return None;
    }
    // SAFETY: `s` 已确认为 CFString，长度查询无前置条件。
    let len = unsafe { CFStringGetLength(s) };
    if len == 0 {
        return Some(String::new());
    }
    // SAFETY: UTF-8 为受支持编码，返回非负的编码字节数上界。
    let max = unsafe { CFStringGetMaximumSizeForEncoding(len, kCFStringEncodingUTF8) };
    let Ok(max) = usize::try_from(max) else {
        return None;
    };
    if max > max_bytes {
        return None;
    }
    let mut buf = vec![0u8; max];
    let mut used: CFIndex = 0;
    // SAFETY: `s` 是有效 CFString；缓冲区容量恰为上界 `max`，GetBytes 不越界写；
    // lossByte=0 表示无法完整转换时返回值小于请求长度。
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

        // SAFETY: 守卫持有有效 +1 引用；上界给足以容纳整串。
        let text = unsafe { string_from(guard.string_ref() as CFTypeRef, 1024) };
        assert_eq!(text.as_deref(), Some("gloss 往返回环"));
    }

    #[test]
    fn string_from_rejects_non_string_objects() {
        let guard = cf_data(b"not a string").expect("cf data");

        // SAFETY: 守卫持有有效 +1 引用；上界刻意放到最大，让类型检查成为唯一
        // 可能的拒绝原因。
        let text = unsafe { string_from(guard.data_ref() as CFTypeRef, usize::MAX) };
        assert_eq!(text, None);
    }

    #[test]
    fn string_from_respects_the_byte_ceiling() {
        let guard = cf_string(c"gloss").expect("cf string");

        // SAFETY: 守卫持有有效 +1 引用；上界刻意小于实际需要。
        let text = unsafe { string_from(guard.string_ref() as CFTypeRef, 2) };
        assert_eq!(text, None);
    }

    #[test]
    fn empty_cf_string_converts_to_empty_rust_string() {
        let guard = cf_string(c"").expect("cf string");

        // SAFETY: 守卫持有有效 +1 引用；空串无需缓冲。
        let text = unsafe { string_from(guard.string_ref() as CFTypeRef, 8) };
        assert_eq!(text.as_deref(), Some(""));
    }
}
