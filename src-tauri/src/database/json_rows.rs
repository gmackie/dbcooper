use serde_json::Value;

pub fn obj_str(row: &Value, key: &str) -> String {
    row.get(key)
        .and_then(|v| {
            v.as_str()
                .map(|s| s.to_string())
                .or_else(|| v.as_i64().map(|n| n.to_string()))
                .or_else(|| v.as_f64().map(|n| n.to_string()))
                .or_else(|| v.as_bool().map(|b| b.to_string()))
        })
        .unwrap_or_default()
}

pub fn obj_i64(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_u64().map(|n| n as i64))
                .or_else(|| v.as_f64().map(|n| n as i64))
                .or_else(|| v.as_bool().map(|b| i64::from(b)))
                .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(0)
}

pub fn obj_opt_str(row: &Value, key: &str) -> Option<String> {
    let value = row.get(key)?;
    if value.is_null() {
        return None;
    }
    Some(obj_str(row, key)).filter(|s| !s.is_empty())
}

pub fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

pub fn normalize_filter(filter: &str) -> String {
    filter
        .replace('\u{2018}', "'")
        .replace('\u{2019}', "'")
        .replace('\u{201C}', "\"")
        .replace('\u{201D}', "\"")
        .replace("\\'", "'")
}
