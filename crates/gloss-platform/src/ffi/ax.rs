//! Accessibility（ApplicationServices 的 HIServices）：AX 元素读取接口。

use std::ffi::{CStr, c_void};

use super::cf::{
    Boolean, CFDictionaryCreate, CFDictionaryRef, CFStringRef, CFTypeRef, CfGuard, cf_string,
    kCFAllocatorDefault, kCFBooleanTrue, kCFTypeDictionaryKeyCallBacks,
    kCFTypeDictionaryValueCallBacks,
};

/// HIServices 的 AX 元素句柄：CF 不透明类型，遵守 +1/-1 内存管理规则。
pub(crate) type AXUIElementRef = *mut c_void;

/// 「焦点元素」属性名的稳定字符串值（kAXFocusedUIElementAttribute 的文档
/// 契约，与 System Events/AppleScript 所用同名）。
pub(crate) const FOCUSED_UI_ELEMENT_ATTRIBUTE: &CStr = c"AXFocusedUIElement";
/// 「选中文本」属性名的稳定字符串值（kAXSelectedTextAttribute 同上）。
pub(crate) const SELECTED_TEXT_ATTRIBUTE: &CStr = c"AXSelectedText";
/// AXIsProcessTrustedWithOptions 选项字典的键（kAXTrustedCheckOptionPrompt
/// 的文档契约值）：true 时对未授权进程弹系统授权引导对话框。
pub(crate) const TRUSTED_CHECK_OPTION_PROMPT: &CStr = c"AXTrustedCheckOptionPrompt";

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// 当前进程是否已获辅助功能授权（TCC）。
    fn AXIsProcessTrusted() -> Boolean;
    /// 带选项的授权查询：`options` 含 prompt=true 时对未授权进程弹系统
    /// 授权引导，返回值仍是当前授权状态。
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> Boolean;
    /// 创建 systemwide AX 元素，成功返回 +1 引用。
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    /// 复制元素属性：成功时 `value` 收到 +1 引用，失败时不写入。
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    /// 设置跨进程消息超时（秒）：传 systemwide 元素 = 对**本进程全局**生效
    /// （此后所有元素的同步 AX 调用按此时长超时）；传其它元素只对该元素
    /// 自身生效，不传播到相等的其它对象。设 0：systemwide 重置回系统默认，
    /// 其它元素回落到全局值。超时后同步调用以 kAXErrorCannotComplete 返回，
    /// 不再无限等待远端应用。返回非 0 表示设置失败（如非法值）。
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, timeout: f32) -> i32;
}

/// 当前进程是否已获辅助功能授权（TCC）；纯查询，无前置条件。
pub(crate) fn is_process_trusted() -> bool {
    // SAFETY: 纯查询型 FFI，无前置条件。
    let trusted = unsafe { AXIsProcessTrusted() };
    trusted != 0
}

/// 当前进程是否已获辅助功能授权；未授权时顺带弹系统授权引导对话框
/// （内含「打开系统设置」入口），返回值仍是查询时刻的授权状态。
///
/// 引导对话框每次调用至多弹一个（系统对同一进程的重复请求会自行合并），
/// 调用方应把调用频次控制在启动预检这类低频节点上。
pub(crate) fn is_process_trusted_prompting() -> bool {
    let Some(key) = cf_string(TRUSTED_CHECK_OPTION_PROMPT) else {
        return is_process_trusted();
    };
    // kCFType 字典回调会对键 retain，key 的守卫已覆盖调用全程；
    // 这里不得再对 key.string_ref() 建第二个守卫（会双重释放）。
    let key_ref = key.string_ref() as CFTypeRef;
    // SAFETY: 读 extern static 的地址值——kCFBooleanTrue 是系统框架导出的
    // 常量指针，地址本身稳定，取地址不存在数据竞争。
    let value_ref: CFTypeRef = unsafe { kCFBooleanTrue as CFTypeRef };
    // SAFETY: 键值均为有效 CF 引用，类型回调负责 retain/release。
    let options = unsafe {
        CFDictionaryCreate(
            kCFAllocatorDefault,
            &key_ref,
            &value_ref,
            1,
            &kCFTypeDictionaryKeyCallBacks,
            &kCFTypeDictionaryValueCallBacks,
        )
    };
    if options.is_null() {
        return is_process_trusted();
    }
    let _options = CfGuard::new(options as CFTypeRef);
    // SAFETY: `options` 是刚构造的有效字典引用。
    let trusted = unsafe { AXIsProcessTrustedWithOptions(options) };
    trusted != 0
}

/// 创建 systemwide AX 元素；系统异常时返回 NULL（调用方判空），否则是 +1 引用。
///
/// # Safety
///
/// 无前置条件；非 NULL 返回值必须恰好释放一次（交给 [`super::cf::CfGuard`]）。
pub(crate) unsafe fn create_system_wide() -> AXUIElementRef {
    // SAFETY: 无前置条件。
    unsafe { AXUIElementCreateSystemWide() }
}

/// 复制元素的一个属性值：成功时把 +1 引用写进 `out`，失败时不写入。
///
/// # Safety
///
/// `element` 必须是有效的 AX 元素引用，`attribute` 必须是有效的 CFString 引用；
/// `out` 必须指向可写内存；返回 0 时其中是必须恰好释放一次的 +1 引用。
pub(crate) unsafe fn copy_attribute(
    element: AXUIElementRef,
    attribute: CFStringRef,
    out: *mut CFTypeRef,
) -> i32 {
    // SAFETY: 前置条件由调用方保证。
    unsafe { AXUIElementCopyAttributeValue(element, attribute, out) }
}

/// 设置跨进程消息超时（秒）：传 systemwide 元素 = 对本进程全局生效；
/// 传其它元素只对该元素自身生效。超时后同步调用以 kAXErrorCannotComplete
/// 返回，不再无限等待远端应用。返回非 0 表示设置失败（如非法值）。
///
/// # Safety
///
/// `element` 必须是有效的 AX 元素引用。
pub(crate) unsafe fn set_messaging_timeout(element: AXUIElementRef, timeout: f32) -> i32 {
    // SAFETY: 前置条件由调用方保证。
    unsafe { AXUIElementSetMessagingTimeout(element, timeout) }
}
