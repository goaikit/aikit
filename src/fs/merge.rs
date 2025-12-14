//! File merging logic
//!
//! This module handles file merging operations, including deep JSON merge
//! for .vscode/settings.json files.

use anyhow::Result;
use serde_json::Value;
use std::path::Path;

/// Result of a file merge operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeResult {
    /// File was created (didn't exist)
    Created,
    /// File was merged (existed, merged successfully)
    Merged,
    /// File was overwritten (existed, no merge logic)
    Overwritten,
    /// File was skipped (conflict resolution)
    Skipped,
}

/// Deep merge JSON values
///
/// Merges two JSON values recursively:
/// - Nested objects are merged recursively
/// - Arrays are replaced entirely (not merged)
/// - Scalars are overwritten
pub fn deep_merge_json(base: &mut Value, new: &Value) {
    if let Value::Object(ref mut base_map) = base {
        if let Value::Object(new_map) = new {
            for (key, new_value) in new_map {
                if let Some(base_value) = base_map.get_mut(key) {
                    deep_merge_json(base_value, new_value);
                } else {
                    base_map.insert(key.clone(), new_value.clone());
                }
            }
            return;
        }
    }
    // If not both objects, replace base with new
    *base = new.clone();
}

/// Merge JSON file if it exists, otherwise create it
pub fn merge_json_file<P: AsRef<Path>>(path: P, new_content: &Value) -> Result<MergeResult> {
    let path = path.as_ref();

    if !path.exists() {
        // File doesn't exist, create it
        std::fs::write(path, serde_json::to_string_pretty(new_content)?)?;
        return Ok(MergeResult::Created);
    }

    // File exists, try to merge
    let existing_content = std::fs::read_to_string(path)?;
    let mut existing_json: Value = serde_json::from_str(&existing_content)
        .map_err(|e| anyhow::anyhow!("Invalid JSON in {}: {}", path.display(), e))?;

    deep_merge_json(&mut existing_json, new_content);
    std::fs::write(path, serde_json::to_string_pretty(&existing_json)?)?;

    Ok(MergeResult::Merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_deep_merge_json() {
        let mut base: Value = serde_json::json!({
            "a": 1,
            "b": {
                "c": 2,
                "d": 3
            },
            "e": [1, 2, 3]
        });

        let new: Value = serde_json::json!({
            "b": {
                "c": 4,
                "f": 5
            },
            "e": [4, 5, 6],
            "g": 7
        });

        deep_merge_json(&mut base, &new);

        assert_eq!(base["a"], 1); // Preserved
        assert_eq!(base["b"]["c"], 4); // Overwritten
        assert_eq!(base["b"]["d"], 3); // Preserved
        assert_eq!(base["b"]["f"], 5); // Added
        assert_eq!(base["e"], json!([4, 5, 6])); // Replaced
        assert_eq!(base["g"], 7); // Added
    }
}
