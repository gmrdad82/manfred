use manfred_build::Call;
use serde_json::{Map, Value};
use std::path::Path;

pub fn read_json(base: &Path, path: &str, label: &str) -> Result<Value, String> {
    let file = base.join(path);
    let text = std::fs::read_to_string(&file)
        .map_err(|e| format!("{label}: cannot read {}: {e}", file.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{label}: {} is not JSON: {e}", file.display()))
}

pub fn source_or_own(call: &Call, own: &str) -> Result<Value, String> {
    match call.params.get("source") {
        Some(Value::String(path)) => {
            if call.params.len() > 1 {
                return Err(format!(
                    "params take either `source` or the {own}, not both"
                ));
            }
            read_json(call.base, path, "params.source")
        }
        Some(other) => Err(format!("params.source: expected a path, found {other}")),
        None => Ok(Value::Object(
            call.params
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        )),
    }
}

pub struct Table<'a> {
    pub path: String,
    pub map: &'a Map<String, Value>,
}

impl<'a> Table<'a> {
    pub fn of(value: &'a Value, path: String) -> Result<Table<'a>, String> {
        match value {
            Value::Object(map) => Ok(Table { path, map }),
            other => Err(format!("{path}: expected a table, found {other}")),
        }
    }

    pub fn section(&self, key: &str) -> Result<Table<'a>, String> {
        match self.map.get(key) {
            Some(value) => Table::of(value, key.to_string()),
            None => Err(format!("{key}: missing")),
        }
    }

    pub fn number(&self, key: &str) -> Result<Option<f64>, String> {
        match self.map.get(key) {
            None => Ok(None),
            Some(value) => value
                .as_f64()
                .map(Some)
                .ok_or_else(|| format!("{}.{key}: expected a number, found {value}", self.path)),
        }
    }

    pub fn req(&self, key: &str) -> Result<f64, String> {
        self.number(key)?
            .ok_or_else(|| format!("{}.{key}: missing", self.path))
    }

    pub fn or(&self, key: &str, default: f64) -> Result<f64, String> {
        Ok(self.number(key)?.unwrap_or(default))
    }

    pub fn count(&self, key: &str) -> Result<usize, String> {
        let value = self.req(key)?;
        if value >= 0.0 && value.fract() == 0.0 && value <= 4096.0 {
            Ok(value as usize)
        } else {
            Err(format!(
                "{}.{key}: expected a whole number from 0 to 4096, found {value}",
                self.path
            ))
        }
    }

    pub fn seed(&self, key: &str) -> Result<i64, String> {
        match self.map.get(key) {
            Some(value) => value.as_i64().ok_or_else(|| {
                format!(
                    "{}.{key}: expected a whole number, found {value}",
                    self.path
                )
            }),
            None => Err(format!("{}.{key}: missing", self.path)),
        }
    }

    pub fn triple(&self, key: &str) -> Result<Option<[f64; 3]>, String> {
        let Some(value) = self.map.get(key) else {
            return Ok(None);
        };
        let wrong = || format!("{}.{key}: expected three numbers, found {value}", self.path);
        let Value::Array(items) = value else {
            return Err(wrong());
        };
        let numbers: Vec<f64> = items
            .iter()
            .map(Value::as_f64)
            .collect::<Option<_>>()
            .ok_or_else(wrong)?;
        numbers.try_into().map(Some).map_err(|_| wrong())
    }

    pub fn name(&self, key: &str) -> Result<Option<String>, String> {
        match self.map.get(key) {
            None => Ok(None),
            Some(Value::String(name)) if !name.is_empty() => Ok(Some(name.clone())),
            Some(other) => Err(format!(
                "{}.{key}: expected a name, found {other}",
                self.path
            )),
        }
    }

    pub fn list(&self, key: &str, required: bool) -> Result<Vec<Table<'a>>, String> {
        match self.map.get(key) {
            None if required => Err(format!("{key}: missing")),
            None => Ok(Vec::new()),
            Some(Value::Array(items)) => items
                .iter()
                .enumerate()
                .map(|(k, item)| Table::of(item, format!("{key}[{k}]")))
                .collect(),
            Some(other) => Err(format!("{key}: expected a list, found {other}")),
        }
    }
}
