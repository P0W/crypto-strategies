//! Grid params for Range Breakout - Minimal for speed

use crate::Config;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GridParams {
    pub lookbacks: Vec<usize>,
    pub stop_atrs: Vec<f64>,
    pub target_atrs: Vec<f64>,
}

impl GridParams {
    /// Quick: 3×4×4 = 48 combinations
    pub fn quick() -> Self {
        Self {
            lookbacks: vec![10, 20, 30],
            stop_atrs: vec![0.5, 1.0, 1.5, 2.0],
            target_atrs: vec![1.0, 1.5, 2.0, 3.0],
        }
    }

    /// Full: 6×5×6 = 180 combinations
    pub fn full() -> Self {
        Self {
            lookbacks: vec![5, 10, 15, 20, 30, 50],
            stop_atrs: vec![0.5, 0.75, 1.0, 1.5, 2.0],
            target_atrs: vec![1.0, 1.5, 2.0, 2.5, 3.0, 4.0],
        }
    }

    pub fn generate_configs(&self, base: &Config) -> Vec<Config> {
        use itertools::iproduct;

        iproduct!(&self.lookbacks, &self.stop_atrs, &self.target_atrs)
            .filter_map(|(lb, stop, target)| {
                if target <= stop {
                    return None;
                }

                let mut config = base.clone();
                if let Some(obj) = config.strategy.as_object_mut() {
                    obj.insert("lookback".to_string(), serde_json::json!(lb));
                    obj.insert("stop_atr".to_string(), serde_json::json!(stop));
                    obj.insert("target_atr".to_string(), serde_json::json!(target));
                }
                Some(config)
            })
            .collect()
    }

    pub fn total_combinations(&self) -> usize {
        use itertools::iproduct;
        iproduct!(&self.lookbacks, &self.stop_atrs, &self.target_atrs)
            .filter(|(_, stop, target)| target > stop)
            .count()
    }
}
