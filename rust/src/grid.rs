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
