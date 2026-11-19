//! 系统剪贴板文本写入的平台中立共享实现：剪贴板写入端口（ClipboardWriter）
//! 的实现两侧逐字同一份，收进共享层单源，平台目录不再各持镜像。
//!
//! 写入走跨平台兜底库（arboard）：其两侧面就是各自系统剪贴板接口的文本格
//! 式封装，且模拟复制兜底的恢复序列已用同一库写回，写入口径一致；不为等
//! 价实现手写不安全全局内存分配（质量条目“整段不安全设计优先换用安全封
//! 装”）。图像不走兜底库——其读图无尺寸设界且不认位图家族——取材面留在
//! 各平台剪贴板模块，另走各自的系统接口。返回结果而不中止或内部消化：写
//! 剪贴板可能失败（板被占用等环境态），调用方按失败记警告降级，浮层不弹
//! 错——复制是顺手功能不是任务出口。
//!
//! 与取材侧分属两个方向：取材读板、这条写板；兜底取材“只读不写”的红线
//! 不受影响——写板只发生在用户显式点击复制的会话内，写入的是注疏产物（
//! 非用户原文）。写板推进板代数（变更计数／序列号）会被哨兵观察到一次板
//! 变更，但纯文本写不含图像风格，图像存在性判定不命中，哨兵不会为它触发
//! 取材——两条通路在类型判定处天然互斥。

use gloss_core::ports::ClipboardWriter;

pub struct SystemClipboardWriter;

impl ClipboardWriter for SystemClipboardWriter {
    fn copy_text(&self, text: &str) -> Result<(), String> {
        arboard::Clipboard::new()
            .and_then(|mut clipboard| clipboard.set_text(text.to_owned()))
            .map_err(|err| err.to_string())
    }
}
