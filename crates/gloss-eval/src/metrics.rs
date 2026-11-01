//! 延迟采样器：记录一次评测运行的逐例端到端耗时样本，折算分位数
//! 供报告与趋势消费。本模块是纯采样器，不读环境、不落盘。

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Latency {
    samples_ms: Vec<f64>,
}

impl Latency {
    pub fn record(&mut self, ms: f64) {
        self.samples_ms.push(ms);
    }

    pub fn count(&self) -> usize {
        self.samples_ms.len()
    }

    pub fn percentile(&mut self, p: f64) -> Option<f64> {
        if self.samples_ms.is_empty() {
            return None;
        }
        self.samples_ms.sort_by(|a, b| a.total_cmp(b));
        let index = (p / 100.0) * (self.samples_ms.len() - 1) as f64;
        let lower = index.floor() as usize;
        let upper = index.ceil() as usize;
        let weight = index - lower as f64;
        Some(self.samples_ms[lower] * (1.0 - weight) + self.samples_ms[upper] * weight)
    }
}

#[cfg(test)]
mod tests {
    use super::Latency;

    #[test]
    fn latency_percentiles_interpolate() {
        let mut latency = Latency::default();
        assert_eq!(latency.percentile(50.0), None);
        for ms in [10.0, 20.0, 30.0, 40.0] {
            latency.record(ms);
        }
        assert_eq!(latency.percentile(50.0), Some(25.0));
        assert_eq!(latency.percentile(95.0), Some(38.5));
        assert_eq!(latency.percentile(0.0), Some(10.0));
        assert_eq!(latency.percentile(100.0), Some(40.0));
    }
}
