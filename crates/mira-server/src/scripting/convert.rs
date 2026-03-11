//! Conversion between Rhai Dynamic values and serde_json::Value.

use rhai::Dynamic;
use serde_json::Value;

/// Convert a serde_json::Value to a Rhai Dynamic.
pub fn value_to_dynamic(value: Value) -> Dynamic {
    match value {
        Value::Null => Dynamic::UNIT,
        Value::Bool(b) => Dynamic::from(b),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Dynamic::from(i)
            } else if let Some(f) = n.as_f64() {
                Dynamic::from(f)
            } else {
                Dynamic::UNIT
            }
        }
        Value::String(s) => Dynamic::from(s),
        Value::Array(arr) => {
            let rhai_arr: rhai::Array = arr.into_iter().map(value_to_dynamic).collect();
            Dynamic::from(rhai_arr)
        }
        Value::Object(map) => {
            let mut rhai_map = rhai::Map::new();
            for (k, v) in map {
                rhai_map.insert(k.into(), value_to_dynamic(v));
            }
            Dynamic::from(rhai_map)
        }
    }
}

/// Convert a Rhai Dynamic to a serde_json::Value.
pub fn dynamic_to_value(d: Dynamic) -> Value {
    if d.is_unit() {
        Value::Null
    } else if d.is::<bool>() {
        Value::Bool(d.cast::<bool>())
    } else if d.is::<i64>() {
        Value::Number(d.cast::<i64>().into())
    } else if d.is::<f64>() {
        serde_json::Number::from_f64(d.cast::<f64>())
            .map(Value::Number)
            .unwrap_or(Value::Null)
    } else if d.is::<String>() {
        Value::String(d.cast::<String>())
    } else if d.is::<rhai::ImmutableString>() {
        Value::String(d.cast::<rhai::ImmutableString>().to_string())
    } else if d.is::<rhai::Array>() {
        let arr = d.cast::<rhai::Array>();
        Value::Array(arr.into_iter().map(dynamic_to_value).collect())
    } else if d.is::<rhai::Map>() {
        let map = d.cast::<rhai::Map>();
        let obj: serde_json::Map<String, Value> = map
            .into_iter()
            .map(|(k, v)| (k.to_string(), dynamic_to_value(v)))
            .collect();
        Value::Object(obj)
    } else {
        Value::String(format!("{d}"))
    }
}

/// Convert a serializable Rust value to Rhai Dynamic via serde_json.
/// Used by bindings to convert tool output to Rhai values.
pub fn to_dynamic<T: serde::Serialize>(value: &T) -> Result<Dynamic, String> {
    let json = serde_json::to_value(value).map_err(|e| e.to_string())?;
    Ok(value_to_dynamic(json))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_null() {
        let d = value_to_dynamic(Value::Null);
        assert!(d.is_unit());
        assert_eq!(dynamic_to_value(d), Value::Null);
    }

    #[test]
    fn roundtrip_bool() {
        let d = value_to_dynamic(Value::Bool(true));
        assert_eq!(dynamic_to_value(d), Value::Bool(true));
    }

    #[test]
    fn roundtrip_int() {
        let d = value_to_dynamic(serde_json::json!(42));
        assert_eq!(dynamic_to_value(d), serde_json::json!(42));
    }

    #[test]
    fn roundtrip_float() {
        let d = value_to_dynamic(serde_json::json!(3.14));
        let v = dynamic_to_value(d);
        assert!(v.as_f64().unwrap() - 3.14 < f64::EPSILON);
    }

    #[test]
    fn roundtrip_string() {
        let d = value_to_dynamic(serde_json::json!("hello"));
        assert_eq!(dynamic_to_value(d), serde_json::json!("hello"));
    }

    #[test]
    fn roundtrip_array() {
        let input = serde_json::json!([1, "two", true]);
        let d = value_to_dynamic(input.clone());
        assert_eq!(dynamic_to_value(d), input);
    }

    #[test]
    fn roundtrip_map() {
        let input = serde_json::json!({"a": 1, "b": "two"});
        let d = value_to_dynamic(input.clone());
        assert_eq!(dynamic_to_value(d), input);
    }

    #[test]
    fn nested_structure() {
        let input = serde_json::json!({
            "results": [
                {"name": "foo", "line": 10},
                {"name": "bar", "line": 20}
            ],
            "total": 2
        });
        let d = value_to_dynamic(input.clone());
        assert_eq!(dynamic_to_value(d), input);
    }
}
