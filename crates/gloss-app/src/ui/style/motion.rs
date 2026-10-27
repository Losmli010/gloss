//! 动效时长与节拍：浮层出现动画、呼吸与微光脉冲周期、帧节拍的定稿。
//!
//! 时间量纲不随空间单位缩放（与字号正交），常数收编在此统一登记：
//! 视图不再出现时长字面量，动效调档只看本文件。

use std::time::Duration;

pub const APPEAR: f32 = 0.18;

pub const PULSE_PERIOD: f64 = 1.2;

pub const PULSE_PHASE_STEP: f64 = 0.9;

pub const FRAME_BEAT: Duration = Duration::from_millis(16);
