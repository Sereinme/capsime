//! 用 UI Automation 读「输入位置」（插入点）——给不创建 Win32 caret 的现代程序用。
//!
//! ## 为什么需要它
//!
//! `GetGUIThreadInfo` 读的是 **Win32 caret**；浏览器（Firefox/Chromium）、Electron、
//! WinUI 这类程序把光标画在自己界面里，从不调 `CreateCaret`，于是只能退回鼠标位置。
//! 但它们几乎都实现了 UI Automation 的 `TextPattern`（无障碍接口）：`GetSelection()`
//! 返回的文本范围就是插入点所在的位置（有多个范围时取最后一个——选区末端的"活动端"
//! 才是插入点）；`TextPattern2.GetCaretRange()` 则直接给出插入点，即使当前有选区。
//!
//! ## 为什么手写 COM 接口而不是加依赖
//!
//! windows-sys 只生成 UIA 的 **provider 侧** flat API（`TextRange_*` 那批），没有客户端
//! 的 `IUIAutomation` 等 COM 接口；引入官方 `windows` crate 又会带进一整套元 crate。
//! 这里一共只用到 6 个方法，所以按 vtable 槽位手写最小接口。**槽位顺序与 IID 逐条取自
//! Windows SDK 10.0.16299 的 `um/UIAutomationClient.h`**（用脚本按 Vtbl 结构体定义顺序
//! 提取，槽位号含 3 个 IUnknown 成员）：
//!
//! | 接口 | IID | 用到的方法（槽位） |
//! | --- | --- | --- |
//! | `IUIAutomation` | `30cbe57d-d9d0-452a-ab13-7ac5ac4825ee` | `GetFocusedElement`(8) |
//! | `IUIAutomation2` | `34723aff-0c9d-49d0-9896-7ab52df8cd8a` | `put_ConnectionTimeout`(61)、`put_TransactionTimeout`(63) |
//! | `IUIAutomationElement` | `d22108aa-8ac5-49a5-837b-37bbb3d7591e` | `GetCurrentPatternAs`(14)、`get_CurrentControlType`(21) |
//! | `IUIAutomationTextPattern` | `32eba289-3583-42c9-9c59-3b6d9a1e9b6a` | `GetSelection`(5) |
//! | `IUIAutomationTextPattern2` | `506a921a-fcc9-409f-b23b-37eb74106872` | `GetCaretRange`(10) |
//! | `IUIAutomationTextRange` | `a543cc6a-f4ae-494b-8239-c814481187a8` | `GetBoundingRectangles`(10) |
//! | `IUIAutomationTextRangeArray` | `ce4ae76a-e717-4c98-81ea-47371d028eb6` | `get_Length`(3)、`GetElement`(4) |
//!
//! ## 卡死防护与线程
//!
//! 跨进程的 UIA 调用可能被前台程序拖住，所以连接与事务超时都收紧到
//! [`TIMEOUT_MS`]；调用要放在**工作线程**（切换线程）上，绝不碰钩子线程。
//! 线程首次调用前会 `CoInitializeEx(MTA)`（MTA 避免跨套间编组）。

use std::cell::Cell;
use std::ffi::c_void;

use windows_sys::Win32::Foundation::{POINT, RECT};
use windows_sys::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, SAFEARRAY,
};
use windows_sys::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayDestroy, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows_sys::core::{GUID, HRESULT};

use crate::log;

const CLSID_UIA: GUID = GUID::from_u128(0xff48dba4_60ef_4201_aa87_54103eef594e);
const IID_UIA: GUID = GUID::from_u128(0x30cbe57d_d9d0_452a_ab13_7ac5ac4825ee);
const IID_UIA2: GUID = GUID::from_u128(0x34723aff_0c9d_49d0_9896_7ab52df8cd8a);
const IID_TEXT_PATTERN: GUID = GUID::from_u128(0x32eba289_3583_42c9_9c59_3b6d9a1e9b6a);
const IID_TEXT_PATTERN2: GUID = GUID::from_u128(0x506a921a_fcc9_409f_b23b_37eb74106872);

/// UIA 的 `TextPattern` / `TextPattern2` 模式 ID（`UIA_TextPatternId` 等）。
const TEXT_PATTERN_ID: i32 = 10014;
const TEXT_PATTERN2_ID: i32 = 10024;

/// 跨进程调用的超时（毫秒）：前台程序卡住时，最坏情况会给切换多拖这么久。
const TIMEOUT_MS: i32 = 250;
/// 有的程序会往矩形里塞垃圾值，超过这个量级一律不认（亿级像素）。
const MAX_COORD: f64 = 1.0e6;

/// 一个最小 COM 指针包装：按 vtable 槽位取函数指针，Drop 时 Release。
struct Com(*mut c_void);

impl Com {
    /// 取 vtable 第 `index` 个槽位并转成需要的函数指针类型。
    ///
    /// # Safety
    /// `index` 必须与真实接口的 vtable 布局一致（见模块文档里的核对表）。
    unsafe fn slot<T>(&self, index: usize) -> T {
        let vtbl = unsafe { *(self.0 as *const *const *const c_void) };
        unsafe { std::mem::transmute_copy(&*vtbl.add(index)) }
    }

    /// `IUnknown::QueryInterface`（槽位 0）。
    unsafe fn query(&self, iid: &GUID) -> Option<Com> {
        type Qi = unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT;
        let query: Qi = unsafe { self.slot(0) };
        let mut out: *mut c_void = std::ptr::null_mut();
        let hr = unsafe { query(self.0, iid, &mut out) };
        (hr >= 0 && !out.is_null()).then_some(Com(out))
    }
}

impl Drop for Com {
    fn drop(&mut self) {
        if self.0.is_null() {
            return;
        }
        type Release = unsafe extern "system" fn(*mut c_void) -> u32;
        let release: Release = unsafe { self.slot(2) };
        unsafe { release(self.0) };
    }
}

thread_local! {
    /// 本线程是否已经初始化过 COM（None = 还没试过）。
    static COM_READY: Cell<Option<bool>> = const { Cell::new(None) };
}

/// 线程首次调用时初始化 COM（MTA，避免跨套间编组），之后复用。
///
/// 试过"每次查询后 `CoUninitialize` 把套间还回去"：没用——多出来的
/// 8 个线程与约 7 MB 工作集是 `uiautomationcore` 自己的连接/回调线程，
/// 只要用过一次 UIA 就会常驻（README 的性能表里如实记录）。
fn ensure_com() -> bool {
    COM_READY.with(|slot| {
        if let Some(ok) = slot.get() {
            return ok;
        }
        let hr = unsafe { CoInitializeEx(std::ptr::null(), COINIT_MULTITHREADED as u32) };
        // S_OK / S_FALSE = 可用（本次或别人初始化过）；
        // RPC_E_CHANGED_MODE（0x8001_0106）= 该线程已是别的套间，COM 照样能用。
        let ok = hr >= 0 || hr == 0x8001_0106u32 as i32;
        if !ok {
            log::debug(&format!("UIA：CoInitializeEx 失败（0x{:08X}）", hr as u32));
        }
        slot.set(Some(ok));
        ok
    })
}

/// 需要 COM 的查询入口：没初始化成功就直接走兜底值。
fn with_com<T>(fallback: impl FnOnce() -> T, query: impl FnOnce() -> T) -> T {
    if !ensure_com() {
        return fallback();
    }
    query()
}

/// 一次 UIA 会话能拿到的锚点候选。
///
/// 插入点的取法：`TextPattern2.GetCaretRange`（有选区时也给插入点）→
/// `TextPattern.GetSelection` 的最后一个范围（没有选区时它就是插入点）。
/// 焦点不在文本上、程序不支持、超时，都拿不到（调用方退回鼠标位置）。
#[derive(Clone, Copy, Default)]
pub struct Anchors {
    /// 插入点（插入点矩形的左下角）。
    pub caret: Option<POINT>,
    /// 焦点控件的外框左下角——有些程序（Chromium 系、终端）给了 `TextPattern`
    /// 却不肯给插入点矩形，此时"贴在输入框上"仍比退回鼠标位置更贴近用户预期。
    pub field: Option<POINT>,
}

/// 查一次 UIA，取插入点；拿不到就顺便给出焦点控件外框（只开一次会话）。
pub fn anchors() -> Anchors {
    with_com(Anchors::default, || {
        let mut out = Anchors::default();
        unsafe {
            let Some(uia) = create_uia() else {
                return out;
            };
            set_timeouts(&uia);
            let Some(element) = focused_element(&uia) else {
                return out;
            };
            out.caret = caret_rect_of(&element).map(rect_anchor);
            if out.caret.is_none() {
                out.field = field_point_of(&element);
            }
        }
        out
    })
}

/// 插入点矩形 → 锚点（取矩形底边：卡片挂在光标下方）。
fn rect_anchor(rect: [f64; 4]) -> POINT {
    POINT {
        x: rect[0].round() as i32,
        y: (rect[1] + rect[3].max(0.0)).round() as i32,
    }
}

/// 焦点元素的外框左下角（只认尺寸合理的矩形）。
unsafe fn field_point_of(element: &Com) -> Option<POINT> {
    type GetRect = unsafe extern "system" fn(*mut c_void, *mut RECT) -> HRESULT;
    let get: GetRect = unsafe { element.slot(43) }; // get_CurrentBoundingRectangle
    let mut rect: RECT = unsafe { std::mem::zeroed() };
    if unsafe { get(element.0, &mut rect) } < 0 {
        return None;
    }
    let (width, height) = (rect.right - rect.left, rect.bottom - rect.top);
    if width <= 4 || height <= 4 || (width as f64) > MAX_COORD || (height as f64) > MAX_COORD {
        return None;
    }
    Some(POINT {
        x: rect.left,
        y: rect.bottom,
    })
}

/// 焦点元素的插入点矩形 `[x, y, width, height]`（物理像素；诊断与探针用）。
pub fn caret_rect() -> Option<[f64; 4]> {
    with_com(
        || None,
        || unsafe {
            let uia = create_uia()?;
            set_timeouts(&uia);
            let element = focused_element(&uia)?;
            caret_rect_of(&element)
        },
    )
}

/// 从焦点元素取插入点矩形（含合理性校验）。
unsafe fn caret_rect_of(element: &Com) -> Option<[f64; 4]> {
    let range = unsafe { caret_range(element) }?;
    let rect = unsafe { bounding_rect(&range) }?;
    if rect.iter().any(|v| !v.is_finite() || v.abs() > MAX_COORD) {
        log::debug("UIA：插入点矩形不合理，忽略");
        return None;
    }
    Some(rect)
}

/// 焦点元素的控件类型 ID（诊断用；拿不到返回 `None`）。
pub fn focused_control_type() -> Option<i32> {
    with_com(
        || None,
        || unsafe {
            let uia = create_uia()?;
            let element = focused_element(&uia)?;
            type GetI32 = unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT;
            let get: GetI32 = element.slot(21); // get_CurrentControlType
            let mut value = 0i32;
            (get(element.0, &mut value) >= 0).then_some(value)
        },
    )
}

/// 控件类型 ID → 便于日志阅读的名字（常见几种）。
pub fn control_type_name(id: i32) -> &'static str {
    match id {
        50004 => "Edit",
        50020 => "Text",
        50030 => "Document",
        50032 => "Window",
        50033 => "Pane",
        _ => "其他",
    }
}

unsafe fn create_uia() -> Option<Com> {
    let mut uia: *mut c_void = std::ptr::null_mut();
    let hr = unsafe {
        CoCreateInstance(
            &CLSID_UIA,
            std::ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_UIA,
            &mut uia,
        )
    };
    if hr < 0 || uia.is_null() {
        log::debug(&format!(
            "UIA：CoCreateInstance 失败（0x{:08X}）",
            hr as u32
        ));
        return None;
    }
    Some(Com(uia))
}

/// 收紧跨进程超时（`IUIAutomation2` 起提供）。
unsafe fn set_timeouts(uia: &Com) {
    let Some(uia2) = (unsafe { uia.query(&IID_UIA2) }) else {
        return;
    };
    type PutI32 = unsafe extern "system" fn(*mut c_void, i32) -> HRESULT;
    let put_connection: PutI32 = unsafe { uia2.slot(61) };
    let put_transaction: PutI32 = unsafe { uia2.slot(63) };
    unsafe {
        put_connection(uia2.0, TIMEOUT_MS);
        put_transaction(uia2.0, TIMEOUT_MS);
    }
}

unsafe fn focused_element(uia: &Com) -> Option<Com> {
    type GetFocused = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT;
    let get: GetFocused = unsafe { uia.slot(8) };
    let mut element: *mut c_void = std::ptr::null_mut();
    let hr = unsafe { get(uia.0, &mut element) };
    if hr < 0 || element.is_null() {
        return None;
    }
    Some(Com(element))
}

/// 取插入点所在的文本范围。
unsafe fn caret_range(element: &Com) -> Option<Com> {
    type GetPatternAs =
        unsafe extern "system" fn(*mut c_void, i32, *const GUID, *mut *mut c_void) -> HRESULT;
    let get_pattern: GetPatternAs = unsafe { element.slot(14) };

    // 1. TextPattern2.GetCaretRange：直接给插入点，有选区也不受影响
    let mut pattern2: *mut c_void = std::ptr::null_mut();
    if unsafe {
        get_pattern(
            element.0,
            TEXT_PATTERN2_ID,
            &IID_TEXT_PATTERN2,
            &mut pattern2,
        )
    } >= 0
        && !pattern2.is_null()
    {
        let pattern2 = Com(pattern2);
        type GetCaretRange =
            unsafe extern "system" fn(*mut c_void, *mut i32, *mut *mut c_void) -> HRESULT;
        let get_range: GetCaretRange = unsafe { pattern2.slot(10) };
        let mut active = 0i32;
        let mut range: *mut c_void = std::ptr::null_mut();
        let hr = unsafe { get_range(pattern2.0, &mut active, &mut range) };
        if hr >= 0 && !range.is_null() {
            return Some(Com(range));
        }
    }

    // 2. 退回 TextPattern.GetSelection：取最后一个范围（选区末端的活动端）
    let mut pattern: *mut c_void = std::ptr::null_mut();
    if unsafe { get_pattern(element.0, TEXT_PATTERN_ID, &IID_TEXT_PATTERN, &mut pattern) } < 0
        || pattern.is_null()
    {
        return None;
    }
    let pattern = Com(pattern);
    type GetSelection = unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT;
    let get_selection: GetSelection = unsafe { pattern.slot(5) };
    let mut array: *mut c_void = std::ptr::null_mut();
    if unsafe { get_selection(pattern.0, &mut array) } < 0 || array.is_null() {
        return None;
    }
    let array = Com(array);
    type GetLength = unsafe extern "system" fn(*mut c_void, *mut i32) -> HRESULT;
    type GetElement = unsafe extern "system" fn(*mut c_void, i32, *mut *mut c_void) -> HRESULT;
    let get_length: GetLength = unsafe { array.slot(3) };
    let get_element: GetElement = unsafe { array.slot(4) };
    let mut length = 0i32;
    if unsafe { get_length(array.0, &mut length) } < 0 || length <= 0 {
        return None;
    }
    let mut range: *mut c_void = std::ptr::null_mut();
    if unsafe { get_element(array.0, length - 1, &mut range) } < 0 || range.is_null() {
        return None;
    }
    Some(Com(range))
}

/// `IUIAutomationTextRange::GetBoundingRectangles`，返回最后一段矩形（插入点那一行）。
unsafe fn bounding_rect(range: &Com) -> Option<[f64; 4]> {
    type GetRects = unsafe extern "system" fn(*mut c_void, *mut *mut SAFEARRAY) -> HRESULT;
    let get_rects: GetRects = unsafe { range.slot(10) };
    let mut array: *mut SAFEARRAY = std::ptr::null_mut();
    if unsafe { get_rects(range.0, &mut array) } < 0 || array.is_null() {
        return None;
    }
    let values = unsafe { read_doubles(array) };
    if values.len() < 4 {
        return None;
    }
    let start = values.len() - 4;
    Some([
        values[start],
        values[start + 1],
        values[start + 2],
        values[start + 3],
    ])
}

/// 读出 `SAFEARRAY` 里的 f64 序列并销毁它。
unsafe fn read_doubles(array: *const SAFEARRAY) -> Vec<f64> {
    let mut out = Vec::new();
    let mut lower = 0i32;
    let mut upper = 0i32;
    let bounds_ok = unsafe { SafeArrayGetLBound(array, 1, &mut lower) } >= 0
        && unsafe { SafeArrayGetUBound(array, 1, &mut upper) } >= 0
        && upper >= lower;
    if bounds_ok {
        let mut data: *mut c_void = std::ptr::null_mut();
        if unsafe { SafeArrayAccessData(array, &mut data) } >= 0 && !data.is_null() {
            let count = (upper - lower + 1) as usize;
            out.extend_from_slice(unsafe { std::slice::from_raw_parts(data as *const f64, count) });
            unsafe { SafeArrayUnaccessData(array) };
        }
    }
    unsafe { SafeArrayDestroy(array) };
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_type_names_cover_common_cases() {
        assert_eq!(control_type_name(50004), "Edit");
        assert_eq!(control_type_name(50030), "Document");
        assert_eq!(control_type_name(12345), "其他");
    }
}
