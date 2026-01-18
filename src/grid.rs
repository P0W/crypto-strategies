//! Generic grid search parameter generation
//!
//! Generates all parameter combinations from a grid config for optimization.

use crate::Config;
use std::collections::HashMap;

/// Generate all config combinations from grid parameters
///
/// Takes a base config with a grid section and generates all possible
/// combinations by computing the cartesian product of all grid params.
pub fn generate_grid_configs(config: &Config) -> Vec<Config> {
    let grid = match &config.grid {
        Some(g) if !g.is_empty() => g,
        _ => return vec![config.clone()], // No grid, return base config only
    };

    // Get sorted keys for deterministic ordering
    let mut keys: Vec<&String> = grid.keys().collect();
    keys.sort();

    // Get values for each key
    let values: Vec<&Vec<serde_json::Value>> = keys.iter().map(|k| &grid[*k]).collect();

    // Generate cartesian product indices
    let combos = cartesian_product_indices(&values);

    // Build configs for each combination
    combos
        .into_iter()
        .map(|indices| {
            let mut cfg = config.clone();
            if let Some(obj) = cfg.strategy.as_object_mut() {
                for (i, &idx) in indices.iter().enumerate() {
                    let key = keys[i];
                    let value = &values[i][idx];
                    obj.insert(key.clone(), value.clone());
                }
            }
            cfg
        })
        .collect()
}

/// Generate cartesian product as index vectors
fn cartesian_product_indices(arrays: &[&Vec<serde_json::Value>]) -> Vec<Vec<usize>> {
    if arrays.is_empty() {
        return vec![vec![]];
    }

    let mut result = Vec::new();
    let mut indices = vec![0usize; arrays.len()];

    loop {
        result.push(indices.clone());

        // Increment indices like an odometer
        let mut pos = arrays.len() - 1;
        loop {
            indices[pos] += 1;
            if indices[pos] < arrays[pos].len() {
                break;
            }
            indices[pos] = 0;
            if pos == 0 {
                return result;
            }
            pos -= 1;
        }
    }
}

/// Get total number of grid combinations
pub fn total_combinations(config: &Config) -> usize {
    match &config.grid {
        Some(grid) if !grid.is_empty() => grid.values().map(|v| v.len()).product(),
        _ => 1, // No grid = 1 combination (base config)
    }
}

/// Parse CLI override into grid format
/// Format: "param=val1,val2,val3" or "param=1.0,2.0,3.0"
pub fn parse_grid_override(s: &str) -> Option<(String, Vec<serde_json::Value>)> {
    let parts: Vec<&str> = s.splitn(2, '=').collect();
    if parts.len() != 2 {
        return None;
    }

    let key = parts[0].trim().to_string();
    let values: Vec<serde_json::Value> = parts[1]
        .split(',')
        .filter_map(|v| {
            let v = v.trim();
            // Try parsing as number first, then as string
            if let Ok(n) = v.parse::<i64>() {
                Some(serde_json::json!(n))
            } else if let Ok(n) = v.parse::<f64>() {
                Some(serde_json::json!(n))
            } else if v == "true" {
                Some(serde_json::json!(true))
            } else if v == "false" {
                Some(serde_json::json!(false))
            } else if !v.is_empty() {
                Some(serde_json::json!(v))
            } else {
                None
            }
        })
        .collect();

    if values.is_empty() {
        None
    } else {
        Some((key, values))
    }
}

/// Apply CLI overrides to config grid
pub fn apply_overrides(config: &mut Config, overrides: &[String]) {
    for override_str in overrides {
        if let Some((key, values)) = parse_grid_override(override_str) {
            let grid = config.grid.get_or_insert_with(HashMap::new);
            grid.insert(key, values);
        }
    }
}

/// Extract strategy params from config for reporting
pub fn extract_params(config: &Config) -> HashMap<String, f64> {
    let mut params = HashMap::new();
    if let Some(obj) = config.strategy.as_object() {
        for (k, v) in obj {
            if let Some(n) = v.as_f64() {
                params.insert(k.clone(), n);
            } else if let Some(n) = v.as_i64() {
                params.insert(k.clone(), n as f64);
            } else if let Some(b) = v.as_bool() {
                params.insert(k.clone(), if b { 1.0 } else { 0.0 });
            }
        }
    }
    params
}

/// Format params for display
pub fn format_params(params: &HashMap<String, f64>) -> String {
    let mut items: Vec<String> = params
        .iter()
        .filter(|(k, _)| !k.starts_with('_')) // Skip internal params
        .map(|(k, v)| {
            // Format nicely based on value
            if v.fract() == 0.0 && *v < 1000.0 {
                format!("{}={}", k, *v as i64)
            } else {
                format!("{}={:.2}", k, v)
            }
        })
        .collect();
    items.sort();
    items.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{BacktestConfig, ExchangeConfig, TaxConfig, TradingConfig};

    fn create_test_config(grid: Option<HashMap<String, Vec<serde_json::Value>>>) -> Config {
        Config {
            exchange: ExchangeConfig::default(),
            trading: TradingConfig::default(),
            strategy: serde_json::json!({
                "name": "test",
                "timeframe": "1d",
                "atr_period": 14,
                "stop_atr": 2.0
            }),
            tax: TaxConfig::default(),
            backtest: BacktestConfig::default(),
            grid,
        }
    }

    // ==================== generate_grid_configs Tests ====================

    #[test]
    fn test_generate_grid_configs_no_grid() {
        let config = create_test_config(None);
        let configs = generate_grid_configs(&config);

        assert_eq!(configs.len(), 1);
        assert_eq!(configs[0].strategy_name(), "test");
    }

    #[test]
    fn test_generate_grid_configs_empty_grid() {
        let config = create_test_config(Some(HashMap::new()));
        let configs = generate_grid_configs(&config);

        assert_eq!(configs.len(), 1);
    }

    #[test]
    fn test_generate_grid_configs_single_param() {
        let mut grid = HashMap::new();
        grid.insert(
            "atr_period".to_string(),
            vec![
                serde_json::json!(10),
                serde_json::json!(14),
                serde_json::json!(20),
            ],
        );

        let config = create_test_config(Some(grid));
        let configs = generate_grid_configs(&config);

        assert_eq!(configs.len(), 3);

        // Verify each config has the correct atr_period
        let atr_periods: Vec<i64> = configs
            .iter()
            .filter_map(|c| c.strategy.get("atr_period").and_then(|v| v.as_i64()))
            .collect();
        assert!(atr_periods.contains(&10));
        assert!(atr_periods.contains(&14));
        assert!(atr_periods.contains(&20));
    }

    #[test]
    fn test_generate_grid_configs_multiple_params() {
        let mut grid = HashMap::new();
        grid.insert(
            "atr_period".to_string(),
            vec![serde_json::json!(10), serde_json::json!(14)],
        );
        grid.insert(
            "stop_atr".to_string(),
            vec![
                serde_json::json!(1.5),
                serde_json::json!(2.0),
                serde_json::json!(2.5),
            ],
        );

        let config = create_test_config(Some(grid));
        let configs = generate_grid_configs(&config);

        // 2 x 3 = 6 combinations
        assert_eq!(configs.len(), 6);
    }

    #[test]
    fn test_generate_grid_configs_preserves_base_params() {
        let mut grid = HashMap::new();
        grid.insert("atr_period".to_string(), vec![serde_json::json!(10)]);

        let config = create_test_config(Some(grid));
        let configs = generate_grid_configs(&config);

        // Verify base params are preserved
        assert_eq!(configs[0].strategy_name(), "test");
        assert_eq!(configs[0].timeframe(), "1d");
    }

    // ==================== total_combinations Tests ====================

    #[test]
    fn test_total_combinations_no_grid() {
        let config = create_test_config(None);
        assert_eq!(total_combinations(&config), 1);
    }

    #[test]
    fn test_total_combinations_empty_grid() {
        let config = create_test_config(Some(HashMap::new()));
        assert_eq!(total_combinations(&config), 1);
    }

    #[test]
    fn test_total_combinations_single_param() {
        let mut grid = HashMap::new();
        grid.insert(
            "atr_period".to_string(),
            vec![
                serde_json::json!(10),
                serde_json::json!(14),
                serde_json::json!(20),
            ],
        );

        let config = create_test_config(Some(grid));
        assert_eq!(total_combinations(&config), 3);
    }

    #[test]
    fn test_total_combinations_multiple_params() {
        let mut grid = HashMap::new();
        grid.insert(
            "atr_period".to_string(),
            vec![serde_json::json!(10), serde_json::json!(14)],
        );
        grid.insert(
            "stop_atr".to_string(),
            vec![
                serde_json::json!(1.5),
                serde_json::json!(2.0),
                serde_json::json!(2.5),
            ],
        );
        grid.insert(
            "target_atr".to_string(),
            vec![serde_json::json!(3.0), serde_json::json!(4.0)],
        );

        let config = create_test_config(Some(grid));
        // 2 x 3 x 2 = 12
        assert_eq!(total_combinations(&config), 12);
    }

    // ==================== parse_grid_override Tests ====================

    #[test]
    fn test_parse_grid_override_integers() {
        let result = parse_grid_override("atr_period=10,14,20");

        assert!(result.is_some());
        let (key, values) = result.unwrap();
        assert_eq!(key, "atr_period");
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], serde_json::json!(10));
        assert_eq!(values[1], serde_json::json!(14));
        assert_eq!(values[2], serde_json::json!(20));
    }

    #[test]
    fn test_parse_grid_override_floats() {
        let result = parse_grid_override("stop_atr=1.5,2.0,2.5");

        assert!(result.is_some());
        let (key, values) = result.unwrap();
        assert_eq!(key, "stop_atr");
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], serde_json::json!(1.5));
        assert_eq!(values[1], serde_json::json!(2.0));
        assert_eq!(values[2], serde_json::json!(2.5));
    }

    #[test]
    fn test_parse_grid_override_booleans() {
        let result = parse_grid_override("use_trailing=true,false");

        assert!(result.is_some());
        let (key, values) = result.unwrap();
        assert_eq!(key, "use_trailing");
        assert_eq!(values.len(), 2);
        assert_eq!(values[0], serde_json::json!(true));
        assert_eq!(values[1], serde_json::json!(false));
    }

    #[test]
    fn test_parse_grid_override_strings() {
        let result = parse_grid_override("strategy=trend,momentum,mean_reversion");

        assert!(result.is_some());
        let (key, values) = result.unwrap();
        assert_eq!(key, "strategy");
        assert_eq!(values.len(), 3);
        assert_eq!(values[0], serde_json::json!("trend"));
        assert_eq!(values[1], serde_json::json!("momentum"));
        assert_eq!(values[2], serde_json::json!("mean_reversion"));
    }

    #[test]
    fn test_parse_grid_override_single_value() {
        let result = parse_grid_override("atr_period=14");

        assert!(result.is_some());
        let (key, values) = result.unwrap();
        assert_eq!(key, "atr_period");
        assert_eq!(values.len(), 1);
        assert_eq!(values[0], serde_json::json!(14));
    }

    #[test]
    fn test_parse_grid_override_invalid_format() {
        assert!(parse_grid_override("no_equals_sign").is_none());
        assert!(parse_grid_override("key=").is_none());
        assert!(parse_grid_override("").is_none());
    }

    #[test]
    fn test_parse_grid_override_with_whitespace() {
        let result = parse_grid_override(" atr_period = 10 , 14 , 20 ");

        assert!(result.is_some());
        let (key, values) = result.unwrap();
        assert_eq!(key, "atr_period");
        assert_eq!(values.len(), 3);
    }

    // ==================== apply_overrides Tests ====================

    #[test]
    fn test_apply_overrides_creates_grid() {
        let mut config = create_test_config(None);
        assert!(config.grid.is_none());

        apply_overrides(&mut config, &["atr_period=10,14".to_string()]);

        assert!(config.grid.is_some());
        let grid = config.grid.as_ref().unwrap();
        assert!(grid.contains_key("atr_period"));
        assert_eq!(grid["atr_period"].len(), 2);
    }

    #[test]
    fn test_apply_overrides_adds_to_existing_grid() {
        let mut grid = HashMap::new();
        grid.insert("stop_atr".to_string(), vec![serde_json::json!(1.5)]);
        let mut config = create_test_config(Some(grid));

        apply_overrides(&mut config, &["atr_period=10,14".to_string()]);

        let grid = config.grid.as_ref().unwrap();
        assert!(grid.contains_key("stop_atr"));
        assert!(grid.contains_key("atr_period"));
    }

    #[test]
    fn test_apply_overrides_multiple() {
        let mut config = create_test_config(None);

        apply_overrides(
            &mut config,
            &[
                "atr_period=10,14".to_string(),
                "stop_atr=1.5,2.0".to_string(),
            ],
        );

        let grid = config.grid.as_ref().unwrap();
        assert_eq!(grid.len(), 2);
    }

    #[test]
    fn test_apply_overrides_overwrites_existing() {
        let mut grid = HashMap::new();
        grid.insert("atr_period".to_string(), vec![serde_json::json!(5)]);
        let mut config = create_test_config(Some(grid));

        apply_overrides(&mut config, &["atr_period=10,14,20".to_string()]);

        let grid = config.grid.as_ref().unwrap();
        assert_eq!(grid["atr_period"].len(), 3);
    }

    // ==================== extract_params Tests ====================

    #[test]
    fn test_extract_params() {
        let config = create_test_config(None);
        let params = extract_params(&config);

        assert!(params.contains_key("atr_period"));
        assert_eq!(params["atr_period"], 14.0);
        assert!(params.contains_key("stop_atr"));
        assert_eq!(params["stop_atr"], 2.0);
    }

    #[test]
    fn test_extract_params_booleans() {
        let mut config = create_test_config(None);
        config.strategy = serde_json::json!({
            "name": "test",
            "timeframe": "1d",
            "use_trailing": true,
            "enable_filter": false
        });

        let params = extract_params(&config);

        assert!(params.contains_key("use_trailing"));
        assert_eq!(params["use_trailing"], 1.0);
        assert!(params.contains_key("enable_filter"));
        assert_eq!(params["enable_filter"], 0.0);
    }

    #[test]
    fn test_extract_params_mixed_types() {
        let mut config = create_test_config(None);
        config.strategy = serde_json::json!({
            "name": "test",
            "timeframe": "1d",
            "atr_period": 14,
            "stop_atr": 2.5,
            "use_trailing": true,
            "description": "some string"  // Should be ignored
        });

        let params = extract_params(&config);

        assert!(params.contains_key("atr_period"));
        assert!(params.contains_key("stop_atr"));
        assert!(params.contains_key("use_trailing"));
        assert!(!params.contains_key("description"));
        assert!(!params.contains_key("name"));
        assert!(!params.contains_key("timeframe"));
    }

    // ==================== format_params Tests ====================

    #[test]
    fn test_format_params_integers() {
        let mut params = HashMap::new();
        params.insert("atr_period".to_string(), 14.0);
        params.insert("lookback".to_string(), 20.0);

        let formatted = format_params(&params);

        assert!(formatted.contains("atr_period=14"));
        assert!(formatted.contains("lookback=20"));
    }

    #[test]
    fn test_format_params_floats() {
        let mut params = HashMap::new();
        params.insert("stop_atr".to_string(), 2.5);
        params.insert("target_atr".to_string(), 5.25);

        let formatted = format_params(&params);

        assert!(formatted.contains("stop_atr=2.50"));
        assert!(formatted.contains("target_atr=5.25"));
    }

    #[test]
    fn test_format_params_skips_internal() {
        let mut params = HashMap::new();
        params.insert("atr_period".to_string(), 14.0);
        params.insert("_internal".to_string(), 100.0);
        params.insert("_optimization".to_string(), 50.0);

        let formatted = format_params(&params);

        assert!(formatted.contains("atr_period=14"));
        assert!(!formatted.contains("_internal"));
        assert!(!formatted.contains("_optimization"));
    }

    #[test]
    fn test_format_params_sorted() {
        let mut params = HashMap::new();
        params.insert("z_param".to_string(), 1.0);
        params.insert("a_param".to_string(), 2.0);
        params.insert("m_param".to_string(), 3.0);

        let formatted = format_params(&params);

        // Should be sorted alphabetically
        let a_pos = formatted.find("a_param").unwrap();
        let m_pos = formatted.find("m_param").unwrap();
        let z_pos = formatted.find("z_param").unwrap();

        assert!(a_pos < m_pos);
        assert!(m_pos < z_pos);
    }

    #[test]
    fn test_format_params_empty() {
        let params = HashMap::new();
        let formatted = format_params(&params);
        assert!(formatted.is_empty());
    }
}
