pub fn type_name<T>(_: &T) -> String {
    std::any::type_name::<T>().to_string()
}
