fn main() {
    // M1-T1 冒烟：验证 app → core + platform 依赖接通（M1-T3 起由 winit 事件循环取代）
    println!(
        "Hello, Gloss! core={}, platform={}",
        gloss_core::core_smoke_marker(),
        gloss_platform::platform_smoke_marker()
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn hello_world() {
        assert_eq!(gloss_platform::platform_smoke_marker(), 2);
    }
}
