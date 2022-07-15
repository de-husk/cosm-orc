use backtrace::Backtrace;

pub fn type_name<T>(_: &T) -> String {
    std::any::type_name::<T>().to_string()
}

// Finds the first external backtrace
// and return the file_name and file_number
pub fn process_backtrace(b: &Backtrace) -> Option<(String, u64)> {
    for frame in b.frames() {
        let symbol = &frame.symbols()[0];
        let file_name = symbol.filename()?.as_os_str().to_str()?;

        // TODO: Make this less brittle
        if !file_name.contains("/backtrace-") && !file_name.contains("cosm-orc/src/orchestrator") {
            return Some((file_name.to_string(), symbol.lineno()? as u64));
        }
    }
    None
}
