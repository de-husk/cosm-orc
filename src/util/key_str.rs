use serde_json::Value;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

pub fn type_name<T>(_: &T) -> String {
    std::any::type_name::<T>().to_string()
}

pub fn op_key(input_json: &Value) -> Option<String> {
    let (k, _) = input_json.as_object()?.iter().next()?;
    Some(format!("{}#{}", k, hash(input_json.to_string())))
}

pub fn hash(s: String) -> u64 {
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}
